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

from . import encoding, error, pycompat, util
from .i18n import _


if pycompat.iswindows:
    from . import win32


class _emptylocker(object):

    def getwarning(self, l):
        return _("waiting for lock on %r") % l.desc


emptylocker = _emptylocker()


class locker(object):
    """Container for a process that originally created a file lock

    It is constructed by parsing a `namespace:uniqueid` string, where
    `namespace` can be:
    - just a 'hostname' (Windows, non-pid-namespacing POSIX)
    - 'hostname/pid-namespace' (modern linux)

    `uniqueid` can be:
    - just a 'pid' (POSIX)
    - pid/starttime (Windows)
    """
    _currentnamespace = None

    def __init__(self, fromstr, path=None):
        """Create a locker object by parsing a string

        Modern `fromstr` should be a string of the following
        format: `<namespacing part>:<identification part>`
        Supports the following formats:
        - `host:pid`
        - `host/pidnamespace:pid`
        - `host:pid/starttime"""
        self.pidnamespace = None
        self.host = None
        self.pid = None
        self.starttime = None

        if ":" not in fromstr:
            msg = _("malformed lock file")
            hint = ""
            if path is not None:
                msg += " (%s)" % path
                hint = _("run hg debuglocks")
            raise error.MalformedLock(msg, hint=hint)
        ns, uid = fromstr.strip().split(":", 1)

        if "/" in ns:
            self.host, self.pidnamespace = ns.split("/", 1)
        elif ns:
            self.host = ns

        if uid and "/" in uid:
            self.pid, self.starttime = uid.split("/", 2)
        else:
            self.pid = uid

    def __eq__(self, other):
        if other is None or other == emptylocker:
            return False
        if isinstance(other, str):
            return self == locker(other)
        return self.namespace == other.namespace and self.uniqueid == other.uniqueid

    @property
    def namespace(self):
        if self.pidnamespace:
            return self.host + "/" + self.pidnamespace
        return self.host

    @property
    def uniqueid(self):
        if self.starttime is not None:
            return self.pid + "/" + self.starttime
        return self.pid

    @classmethod
    def getcurrentnamespace(cls):
        if cls._currentnamespace is not None:
            return cls._currentnamespace
        result = socket.gethostname()
        if pycompat.ispy3:
            result = result.encode(pycompat.sysstr(encoding.encoding), "replace")
        if pycompat.sysplatform.startswith("linux"):
            try:
                result += "/%x" % os.stat("/proc/self/ns/pid").st_ino
            except OSError as ex:
                if ex.errno not in (errno.ENOENT, errno.EACCES, errno.ENOTDIR):
                    raise
        cls._currentnamespace = result
        return result

    @staticmethod
    def getcurrentid():
        if pycompat.iswindows:
            return "%d/%d" % (util.getpid(), win32.getcurrentprocstarttime())
        return str(util.getpid())

    def issamenamespace(self):
        """Check if the current process is in the same namespace as locker"""
        return self.namespace == self.getcurrentnamespace()

    def isrunning(self):
        """Check if locker is still running"""
        if self.pid is None:
            return False
        pid = int(self.pid)
        starttime = self.starttime and int(self.starttime)
        result = util.testpid(pid)
        if result and pycompat.iswindows and starttime is not None:
            result = starttime == win32.getprocstarttime(pid)
        return result

    def getwarning(self, l):
        """Get a locker's warning string while trying to acquire `l` lock"""
        msg = _("waiting for lock on %s held by process %r on host %r\n")
        msg %= (l.desc, self.pid, self.host)
        return "".join(msg)


def trylock(ui, vfs, lockname, timeout, warntimeout, *args, **kwargs):
    """return an acquired lock or raise an a LockHeld exception

    This function is responsible to issue warnings and or debug messages about
    the held lock while trying to acquires it."""
    l = lock(vfs, lockname, 0, *args, dolock=False, ui=ui, **kwargs)

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
                ui.debug(inst.locker.getwarning(l))
            if delay == warningidx:
                ui.warn(inst.locker.getwarning(l))
            if timeout <= delay:
                raise error.LockHeld(
                    errno.ETIMEDOUT, inst.filename, l.desc, inst.locker
                )
            time.sleep(1)
            delay += 1

    l.delay = delay
    if l.delay:
        msg = _("got lock after %s seconds\n") % l.delay
        if 0 <= warningidx <= l.delay:
            ui.warn(msg)
        else:
            ui.debug(msg)
    if l.acquirefn:
        l.acquirefn()
    return l


class lock(object):
    """An advisory lock held by one process to control access to a set
    of files.  Non-cooperating processes or incorrectly written scripts
    can ignore Mercurial's locking scheme and stomp all over the
    repository, so don't do that.

    Typically used via localrepository.lock() to lock the repository
    store (.hg/store/) or localrepository.wlock() to lock everything
    else under .hg/."""

    # lock is symlink on platforms that support it, file on others.

    # symlink is used because create of directory entry and contents
    # are atomic even over nfs.

    # old-style lock: symlink to pid
    # new-style lock: symlink to hostname:pid

    _namespace = None

    def __init__(
        self,
        vfs,
        file,
        timeout=-1,
        releasefn=None,
        acquirefn=None,
        desc=None,
        inheritchecker=None,
        parentlock=None,
        dolock=True,
        ui=None,
    ):
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
        self.postrelease = []
        self.pid = self._getpid()
        self.ui = ui
        if dolock:
            self.delay = self.lock()
            if self.acquirefn:
                self.acquirefn()

        self._debugmessagesprinted = set([])

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_value, exc_tb):
        self.release()

    def __del__(self):
        if self.held:
            warnings.warn(
                "use lock.release instead of del lock",
                category=DeprecationWarning,
                stacklevel=2,
            )

            # ensure the lock will be removed
            # even if recursive locking did occur
            self.held = 1

        self.release()

    def _getpid(self):
        # wrapper around locker.getcurrentid() to make testing easier
        return locker.getcurrentid()

    def _getlockname(self):
        return "%s:%s" % (locker.getcurrentnamespace(), self.pid)

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
                raise error.LockHeld(
                    errno.ETIMEDOUT, inst.filename, self.desc, inst.locker
                )

    def _trylock(self):
        if self.held:
            self.held += 1
            return
        retry = 5
        while not self.held and retry:
            retry -= 1
            try:
                self.vfs.makelock(self._getlockname(), self.f)
                self.held = 1
            except (OSError, IOError) as why:
                if why.errno == errno.EEXIST:
                    lockfilecontents = self._readlock()
                    if lockfilecontents is None:
                        continue
                    lockerdesc = locker(lockfilecontents, path=self.vfs.join(self.f))

                    # special case where a parent process holds the lock -- this
                    # is different from the pid being different because we do
                    # want the unlock and postrelease functions to be called,
                    # but the lockfile to not be removed.
                    if lockerdesc == self.parentlock:
                        self._parentheld = True
                        self.held = 1
                        return
                    lockerdesc = self._testlock(lockerdesc)
                    if lockerdesc is not None:
                        raise error.LockHeld(
                            errno.EAGAIN, self.vfs.join(self.f), self.desc, lockerdesc
                        )
                else:
                    raise error.LockUnavailable(
                        why.errno, why.strerror, why.filename, self.desc
                    )

        if not self.held:
            # use empty locker to mean "busy for frequent lock/unlock
            # by many processes"
            raise error.LockHeld(
                errno.EAGAIN, self.vfs.join(self.f), self.desc, emptylocker
            )

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

    def _debugprintonce(self, msg):
        """Print debug message only once"""
        if not self.ui or msg in self._debugmessagesprinted:
            return
        self._debugmessagesprinted.add(msg)
        self.ui.debug(msg)

    def _testlock(self, lockerdesc):
        if lockerdesc is None:
            return None
        if not lockerdesc.issamenamespace():
            # this and below debug prints will hopefully help us
            # understand the issue with stale lock files not being
            # cleaned up on Windows (T25415269)
            m = _("locker is not in the same namespace(locker: %r, us: %r)\n")
            m %= (lockerdesc.namespace, locker.getcurrentnamespace())
            self._debugprintonce(m)
            return lockerdesc
        if lockerdesc.isrunning():
            m = _("locker is still running (full unique id: %r)\n")
            m %= (lockerdesc.uniqueid,)
            self._debugprintonce(m)
            return lockerdesc
        # if locker dead, break lock.  must do this with another lock
        # held, or can race and break valid lock.
        try:
            msg = _(
                "trying to removed the stale lock file " "(will acquire %s for that)\n"
            )
            breaklock = self.f + ".break"
            self._debugprintonce(msg % breaklock)
            l = lock(self.vfs, breaklock, timeout=0)
            self.vfs.unlink(self.f)
            l.release()
            self._debugprintonce(_("removed the stale lock file\n"))
        except error.LockError:
            self._debugprintonce(_("failed to remove the stale lock file\n"))
            return lockerdesc

    def testlock(self):
        """return id of locker if lock is valid, else None.

        If old-style lock, we cannot tell what machine locker is on.
        with new-style lock, if locker is on this machine, we can
        see if locker is alive.  If locker is on this machine but
        not alive, we can safely break lock.

        The lock file is only deleted when None is returned.

        """
        lockerdesc = locker(self._readlock(), path=self.vfs.join(self.f))
        return self._testlock(lockerdesc)

    @contextlib.contextmanager
    def inherit(self):
        """context for the lock to be inherited by a Mercurial subprocess.

        Yields a string that will be recognized by the lock in the subprocess.
        Communicating this string to the subprocess needs to be done separately
        -- typically by an environment variable.
        """
        if not self.held:
            raise error.LockInheritanceContractViolation(
                "inherit can only be called while lock is held"
            )
        if self._inherited:
            raise error.LockInheritanceContractViolation(
                "inherit cannot be called while lock is already inherited"
            )
        if self._inheritchecker is not None:
            self._inheritchecker()
        if self.releasefn:
            self.releasefn()
        if self._parentheld:
            lockname = self.parentlock
        else:
            lockname = self._getlockname()
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
