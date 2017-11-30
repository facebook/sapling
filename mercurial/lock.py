# lock.py - simple advisory locking scheme for mercurial
#
# Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import contextlib
import errno
import os
import socket
import time
import warnings

from .i18n import _

from . import (
    encoding,
    error,
    pycompat,
    util,
)

def _getlockprefix():
    """Return a string which is used to differentiate pid namespaces

    It's useful to detect "dead" processes and remove stale locks with
    confidence. Typically it's just hostname. On modern linux, we include an
    extra Linux-specific pid namespace identifier.
    """
    result = socket.gethostname()
    if pycompat.ispy3:
        result = result.encode(pycompat.sysstr(encoding.encoding), 'replace')
    if pycompat.sysplatform.startswith('linux'):
        try:
            result += '/%x' % os.stat('/proc/self/ns/pid').st_ino
        except OSError as ex:
            if ex.errno not in (errno.ENOENT, errno.EACCES, errno.ENOTDIR):
                raise
    return result

def trylock(ui, vfs, lockname, timeout, warntimeout, *args, **kwargs):
    """return an acquired lock or raise an a LockHeld exception

    This function is responsible to issue warnings and or debug messages about
    the held lock while trying to acquires it."""

    def printwarning(printer, locker):
        """issue the usual "waiting on lock" message through any channel"""
        # show more details for new-style locks
        if ':' in locker:
            host, pid = locker.split(":", 1)
            msg = _("waiting for lock on %s held by process %r "
                    "on host %r\n") % (l.desc, pid, host)
        else:
            msg = _("waiting for lock on %s held by %r\n") % (l.desc, locker)
        printer(msg)

    l = lock(vfs, lockname, 0, *args, dolock=False, **kwargs)

    debugidx = 0 if (warntimeout and timeout) else -1
    warningidx = 0
    if not timeout:
        warningidx = -1
    elif warntimeout:
        warningidx = warntimeout

    delay = 0
    while True:
        try:
            l._trylock()
            break
        except error.LockHeld as inst:
            if delay == debugidx:
                printwarning(ui.debug, inst.locker)
            if delay == warningidx:
                printwarning(ui.warn, inst.locker)
            if timeout <= delay:
                raise error.LockHeld(errno.ETIMEDOUT, inst.filename,
                                     l.desc, inst.locker)
            time.sleep(1)
            delay += 1

    l.delay = delay
    if l.delay:
        if 0 <= warningidx <= l.delay:
            ui.warn(_("got lock after %s seconds\n") % l.delay)
        else:
            ui.debug("got lock after %s seconds\n" % l.delay)
    if l.acquirefn:
        l.acquirefn()
    return l

class lock(object):
    '''An advisory lock held by one process to control access to a set
    of files.  Non-cooperating processes or incorrectly written scripts
    can ignore Mercurial's locking scheme and stomp all over the
    repository, so don't do that.

    Typically used via localrepository.lock() to lock the repository
    store (.hg/store/) or localrepository.wlock() to lock everything
    else under .hg/.'''

    # lock is symlink on platforms that support it, file on others.

    # symlink is used because create of directory entry and contents
    # are atomic even over nfs.

    # old-style lock: symlink to pid
    # new-style lock: symlink to hostname:pid

    _host = None

    def __init__(self, vfs, file, timeout=-1, releasefn=None, acquirefn=None,
                 desc=None, inheritchecker=None, parentlock=None,
                 dolock=True):
        self.vfs = vfs
        self.f = file
        self.held = 0
        self.timeout = timeout
        self.releasefn = releasefn
        self.acquirefn = acquirefn
        self.desc = desc
        self._inheritchecker = inheritchecker
        self.parentlock = parentlock
        self._parentheld = False
        self._inherited = False
        self.postrelease  = []
        self.pid = self._getpid()
        if dolock:
            self.delay = self.lock()
            if self.acquirefn:
                self.acquirefn()

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_value, exc_tb):
        self.release()

    def __del__(self):
        if self.held:
            warnings.warn("use lock.release instead of del lock",
                    category=DeprecationWarning,
                    stacklevel=2)

            # ensure the lock will be removed
            # even if recursive locking did occur
            self.held = 1

        self.release()

    def _getpid(self):
        # wrapper around util.getpid() to make testing easier
        return util.getpid()

    def lock(self):
        timeout = self.timeout
        while True:
            try:
                self._trylock()
                return self.timeout - timeout
            except error.LockHeld as inst:
                if timeout != 0:
                    time.sleep(1)
                    if timeout > 0:
                        timeout -= 1
                    continue
                raise error.LockHeld(errno.ETIMEDOUT, inst.filename, self.desc,
                                     inst.locker)

    def _trylock(self):
        if self.held:
            self.held += 1
            return
        if lock._host is None:
            lock._host = _getlockprefix()
        lockname = '%s:%d' % (lock._host, self.pid)
        retry = 5
        while not self.held and retry:
            retry -= 1
            try:
                self.vfs.makelock(lockname, self.f)
                self.held = 1
            except (OSError, IOError) as why:
                if why.errno == errno.EEXIST:
                    locker = self._readlock()
                    if locker is None:
                        continue

                    # special case where a parent process holds the lock -- this
                    # is different from the pid being different because we do
                    # want the unlock and postrelease functions to be called,
                    # but the lockfile to not be removed.
                    if locker == self.parentlock:
                        self._parentheld = True
                        self.held = 1
                        return
                    locker = self._testlock(locker)
                    if locker is not None:
                        raise error.LockHeld(errno.EAGAIN,
                                             self.vfs.join(self.f), self.desc,
                                             locker)
                else:
                    raise error.LockUnavailable(why.errno, why.strerror,
                                                why.filename, self.desc)

        if not self.held:
            # use empty locker to mean "busy for frequent lock/unlock
            # by many processes"
            raise error.LockHeld(errno.EAGAIN,
                                 self.vfs.join(self.f), self.desc, "")

    def _readlock(self):
        """read lock and return its value

        Returns None if no lock exists, pid for old-style locks, and host:pid
        for new-style locks.
        """
        try:
            return self.vfs.readlock(self.f)
        except (OSError, IOError) as why:
            if why.errno == errno.ENOENT:
                return None
            raise

    def _testlock(self, locker):
        if locker is None:
            return None
        try:
            host, pid = locker.split(":", 1)
        except ValueError:
            return locker
        if host != lock._host:
            return locker
        try:
            pid = int(pid)
        except ValueError:
            return locker
        if util.testpid(pid):
            return locker
        # if locker dead, break lock.  must do this with another lock
        # held, or can race and break valid lock.
        try:
            l = lock(self.vfs, self.f + '.break', timeout=0)
            self.vfs.unlink(self.f)
            l.release()
        except error.LockError:
            return locker

    def testlock(self):
        """return id of locker if lock is valid, else None.

        If old-style lock, we cannot tell what machine locker is on.
        with new-style lock, if locker is on this machine, we can
        see if locker is alive.  If locker is on this machine but
        not alive, we can safely break lock.

        The lock file is only deleted when None is returned.

        """
        locker = self._readlock()
        return self._testlock(locker)

    @contextlib.contextmanager
    def inherit(self):
        """context for the lock to be inherited by a Mercurial subprocess.

        Yields a string that will be recognized by the lock in the subprocess.
        Communicating this string to the subprocess needs to be done separately
        -- typically by an environment variable.
        """
        if not self.held:
            raise error.LockInheritanceContractViolation(
                'inherit can only be called while lock is held')
        if self._inherited:
            raise error.LockInheritanceContractViolation(
                'inherit cannot be called while lock is already inherited')
        if self._inheritchecker is not None:
            self._inheritchecker()
        if self.releasefn:
            self.releasefn()
        if self._parentheld:
            lockname = self.parentlock
        else:
            lockname = '%s:%s' % (lock._host, self.pid)
        self._inherited = True
        try:
            yield lockname
        finally:
            if self.acquirefn:
                self.acquirefn()
            self._inherited = False

    def release(self):
        """release the lock and execute callback function if any

        If the lock has been acquired multiple times, the actual release is
        delayed to the last release call."""
        if self.held > 1:
            self.held -= 1
        elif self.held == 1:
            self.held = 0
            if self._getpid() != self.pid:
                # we forked, and are not the parent
                return
            try:
                if self.releasefn:
                    self.releasefn()
            finally:
                if not self._parentheld:
                    try:
                        self.vfs.unlink(self.f)
                    except OSError:
                        pass
            # The postrelease functions typically assume the lock is not held
            # at all.
            if not self._parentheld:
                for callback in self.postrelease:
                    callback()
                # Prevent double usage and help clear cycles.
                self.postrelease = None

def release(*locks):
    for lock in locks:
        if lock is not None:
            lock.release()
