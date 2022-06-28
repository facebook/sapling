# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# lock.py - simple advisory locking scheme for mercurial
#
# Copyright 2005, 2006 Olivia Mackall <olivia@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import copy
import errno
import os
import socket
import time
import warnings

from bindings import lock as nativelock

from . import encoding, error, progress, pycompat, util
from .i18n import _


if pycompat.iswindows:
    from . import win32


class _emptylockinfo(object):
    def getwarning(self, l):
        return _("waiting for lock on %r") % l.desc


emptylockinfo = _emptylockinfo()
defaultlockwaitwarntimeout = 3


class lockinfo(object):
    """Information about who is holding the lock.

    Does not have write side-effect (ex. take or release a lock).
    """

    _currentnamespace = None

    def __init__(self, fromstr, path=None):
        """
        fromstr := namespace + ":" + identification
        namespace := hostname (non-Linux) | hostname + "/" + pid-namespace (Linux)
        identification := pid (non-Windows) | pid + "/" + starttime (Windows)
        """
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
        if other is None or other == emptylockinfo:
            return False
        if isinstance(other, str):
            return self == lockinfo(other)
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
        """Check if the current process is in the same namespace as lockinfo"""
        return self.namespace == self.getcurrentnamespace()

    def isrunning(self):
        """Check if locker process is still running"""
        if self.pid is None:
            return False
        pid = int(self.pid)
        starttime = self.starttime and int(self.starttime)
        result = util.testpid(pid)
        if result and pycompat.iswindows and starttime is not None:
            result = starttime == win32.getprocstarttime(pid)
        return result

    def getwarning(self, l):
        """Get a lockinfo's warning string while trying to acquire `l` lock"""
        msg = _("waiting for lock on %s held by process %r on host %r\n")
        msg %= (l.desc, self.pid, self.host)
        hintmsg = _(
            "(hint: run 'hg debugprocesstree %s' to see related processes)\n"
        ) % (self.pid,)
        return msg + hintmsg

    def __str__(self):
        return _("process %r on host %r") % (self.pid, self.host)


def trylock(ui, vfs, lockname, timeout, warntimeout=None, *args, **kwargs):
    """return an acquired lock or raise an a LockHeld exception

    This function is responsible to issue warnings and or debug messages about
    the held lock while trying to acquire it."""

    debugidx = 0 if (warntimeout and timeout) else -1
    warningidx = 0
    if not timeout:
        warningidx = -1
    elif warntimeout is not None:
        warningidx = warntimeout
    else:
        warningidx = defaultlockwaitwarntimeout

    l = lock(
        vfs,
        lockname,
        timeout,
        *args,
        ui=ui,
        warnattemptidx=warningidx,
        debugattemptidx=debugidx,
        **kwargs,
    )

    if l.delay:
        msg = _("got lock after %s seconds\n") % l.delay
        if 0 <= warningidx <= l.delay:
            ui.warn(msg)
        else:
            ui.debug(msg)

    return l


# Temporary method to dispatch based on devel.lockmode during transition to rust locks.
def lock(*args, ui=None, **kwargs):
    lockclass = pythonlock

    if ui:
        mode = ui.config("devel", "lockmode")
        if mode == "rust_only":
            lockclass = rustlock
        elif mode == "python_and_rust":
            kwargs = copy.copy(kwargs)
            kwargs["andrust"] = True

    return lockclass(*args, ui=ui, **kwargs)


class pythonlock(object):
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

    def __init__(
        self,
        vfs,
        file,
        timeout=-1,
        releasefn=None,
        acquirefn=None,
        desc=None,
        ui=None,
        showspinner=False,
        spinnermsg=None,
        warnattemptidx=None,
        debugattemptidx=None,
        checkdeadlock=True,
        andrust=False,
    ):
        self.vfs = vfs
        self.f = file
        self.held = 0
        self.timeout = timeout
        self.releasefn = releasefn
        self.acquirefn = acquirefn
        self.desc = desc
        self.postrelease = []
        self.pid = self._getpid()
        self.ui = ui
        self.showspinner = showspinner
        self.spinnermsg = spinnermsg
        self.warnattemptidx = warnattemptidx
        self.debugattemptidx = debugattemptidx
        self.checkdeadlock = checkdeadlock
        self._debugmessagesprinted = set([])
        self._lockfd = None
        self.andrust = andrust
        self._rustlock = None

        self.delay = self.lock()
        if self.acquirefn:
            self.acquirefn()

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
        # wrapper around lockinfo.getcurrentid() to make testing easier
        return lockinfo.getcurrentid()

    def _getlockname(self):
        return "%s:%s" % (lockinfo.getcurrentnamespace(), self.pid)

    def lock(self):
        # wrapper around locking to show spinner
        if self.showspinner and self.ui:
            if self.spinnermsg:
                msg = self.spinnermsg
            else:
                msg = _("waiting for the lock to be released")
            spinner = progress.spinner(self.ui, msg)
        else:
            spinner = util.nullcontextmanager()
        with spinner:
            self._dolock()

    def _dolock(self):
        delay = 0
        while True:
            try:
                self._trylock()
                return delay
            except error.LockHeld as inst:
                if self.ui and delay == self.debugattemptidx:
                    self.ui.debug(inst.lockinfo.getwarning(self))
                if self.ui and delay == self.warnattemptidx:
                    self.ui.warn(inst.lockinfo.getwarning(self))

                if self.timeout >= 0 and delay >= self.timeout:
                    raise error.LockHeld(
                        errno.ETIMEDOUT, inst.filename, self.desc, inst.lockinfo
                    )

                time.sleep(1)
                delay += 1

    def _trylock(self):
        if self.held:
            self.held += 1
            return
        assert self._lockfd is None
        retry = 5

        path = self.vfs.join(self.f)
        if (
            util.istest()
            and self.f
            in encoding.environ.get("EDENSCM_TEST_PRETEND_LOCKED", "").split()
        ):
            raise error.LockHeld(errno.EAGAIN, path, self.desc, None)

        while not self.held and retry:
            retry -= 1
            try:
                self._lockfd = self.vfs.makelock(
                    self._getlockname(), self.f, checkdeadlock=self.checkdeadlock
                )
                self.held = 1
            except (OSError, IOError) as why:
                # EEXIST: lockfile exists (Windows)
                # ELOOP: lockfile exists as a symlink (POSIX)
                # EAGAIN: lockfile flock taken by other process (POSIX)
                if why.errno in {errno.EEXIST, errno.ELOOP, errno.EAGAIN}:
                    lockfilecontents = self._readlock()
                    if lockfilecontents is None:
                        continue
                    info = lockinfo(lockfilecontents, path=self.vfs.join(self.f))
                    info = self._testlock(info)
                    if info is not None:
                        raise error.LockHeld(
                            errno.EAGAIN, self.vfs.join(self.f), self.desc, info
                        )

                else:
                    raise error.LockUnavailable(
                        why.errno, why.strerror, why.filename, self.desc
                    )

        if not self.held:
            # use empty lockinfo to mean "busy for frequent lock/unlock
            # by many processes"
            raise error.LockHeld(
                errno.EAGAIN, self.vfs.join(self.f), self.desc, emptylockinfo
            )

        if self.andrust:
            try:
                self._rustlock = rustlock(
                    self.vfs,
                    self.f,
                    timeout=self.timeout,
                    desc=self.desc,
                    ui=self.ui,
                    warnattemptidx=self.warnattemptidx,
                    debugattemptidx=self.debugattemptidx,
                    # don't pass callbacks to avoid double invocation
                    # don't pass spinner to avoid double spinner
                )
            except Exception as err:
                # Avoid invoking python lock's release callback if rust lock acquisition fails.
                self.releasefn = None
                self.release()
                raise err

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

    def _testlock(self, info):
        if info is None:
            return None
        if not info.issamenamespace():
            # this and below debug prints will hopefully help us
            # understand the issue with stale lock files not being
            # cleaned up on Windows (T25415269)
            m = _("locker is not in the same namespace(locker: %r, us: %r)\n")
            m %= (info.namespace, lockinfo.getcurrentnamespace())
            self._debugprintonce(m)
            return info
        # On posix.makelock removes stale locks automatically. So there is no
        # need to remove stale lock here.
        if info.isrunning() or not pycompat.iswindows:
            m = _("locker is still running (full unique id: %r)\n")
            m %= (info.uniqueid,)
            self._debugprintonce(m)
            return info
        # XXX: The below logic is broken since "read + test + unlink" should
        # happen atomically, in a same critical section. Use another lock to
        # only protect "unlink" is not enough.
        #
        # if lockinfo dead, break lock.  must do this with another lock
        # held, or can race and break valid lock.
        try:
            # The "remove dead lock" logic is done by posix.makelock, not here.
            assert pycompat.iswindows
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
            return info

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
                try:
                    self._release()
                    self._lockfd = None
                except OSError:
                    pass
            for callback in self.postrelease:
                callback()
            # Prevent double usage and help clear cycles.
            self.postrelease = None

    def _release(self):
        util.releaselock(self._lockfd, self.vfs.join(self.f))

        if self._rustlock:
            self._rustlock.release()


class rustlock(pythonlock):
    """Delegates to rust implementation of advisory file lock."""

    def _trylock(self):
        if self.held:
            self.held += 1
            return
        assert self._lockfd is None

        path = self.vfs.join(self.f)
        if (
            util.istest()
            and self.f
            in encoding.environ.get("EDENSCM_TEST_PRETEND_LOCKED", "").split()
        ):
            raise error.LockHeld(errno.EAGAIN, path, self.desc, None)

        try:
            self._lockfd = nativelock.pathlock.trylock(
                self.vfs.dirname(path), self.vfs.basename(path), self._getlockname()
            )
            self.held = 1
        except error.LockContendedError as err:
            raise error.LockHeld(
                errno.EAGAIN,
                path,
                self.desc,
                lockinfo(err.args[0], path=path),
            )
        except IOError as err:
            raise error.LockUnavailable(
                err.errno,
                str(err),
                path,
                self.desc,
            )

    def _release(self):
        self._lockfd.unlock()


def islocked(vfs, name):
    try:
        lock(vfs, name, timeout=0, checkdeadlock=False).release()
        return False
    except error.LockHeld:
        return True


def release(*locks):
    for lock in locks:
        if lock is not None:
            lock.release()
