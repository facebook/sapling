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

import errno
import os
import socket
import time
import warnings
from typing import Optional

from bindings import lock as nativelock

from . import encoding, error, perftrace, progress, pycompat, util
from .i18n import _


if pycompat.iswindows:
    from . import win32


class _emptylockinfo:
    def getwarning(self, l):
        return _("waiting for lock on %r") % l.desc


emptylockinfo = _emptylockinfo()
defaultlockwaitwarntimeout = 3


class lockinfo:
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
                hint = _("run @prog@ debuglocks")
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
            "(hint: run '@prog@ debugprocesstree %s' to see related processes)\n"
        ) % (self.pid,)
        return msg + hintmsg

    def __str__(self):
        return _("process %r on host %r") % (self.pid, self.host)


def trylock(
    ui, vfs, lockname, timeout, warntimeout: Optional[int] = None, *args, **kwargs
) -> "lock":
    """return an acquired lock or raise an a LockHeld exception

    This function is responsible to issue warnings and or debug messages about
    the held lock while trying to acquire it."""

    debugsecs = 0 if (warntimeout and timeout) else -1
    warnsecs = 0
    if not timeout:
        warnsecs = -1
    elif warntimeout is not None:
        warnsecs = warntimeout
    else:
        warnsecs = defaultlockwaitwarntimeout

    l = lock(
        vfs,
        lockname,
        timeout,
        *args,
        ui=ui,
        warnsecs=warnsecs,
        debugsecs=debugsecs,
        **kwargs,
    )

    if l.delay:
        msg = _("got lock after %s seconds\n") % l.delay
        if 0 <= warnsecs <= l.delay:
            ui.warn(msg)
        else:
            ui.debug(msg)

    return l


class lock:
    """An advisory lock held by one process to control access to a set
    of files.  Non-cooperating processes or incorrectly written scripts
    can ignore Mercurial's locking scheme and stomp all over the
    repository, so don't do that.

    Typically used via localrepository.lock() to lock the repository
    store (.hg/store/) or localrepository.wlock() to lock everything
    else under .hg/."""

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
        warnsecs=-1,
        debugsecs=-1,
        trylockfn=None,
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
        self.warnsecs = warnsecs
        self.debugsecs = debugsecs
        self.trylockfn = trylockfn
        self._debugmessagesprinted = set([])
        self._rustlock = None

        self.delay = self.lock()
        if self.acquirefn:
            try:
                self.acquirefn()
            except:  # re-raises
                # Release ourself immediately so locks are released in reverse order
                # if acquirefn crashes for second lock in a "with" statement.
                self.release()
                raise

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

    @perftrace.tracefunc("lock")
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
            return self._dolock()

    def _dolock(self):
        delay = 0
        warned = debugged = False

        while True:
            try:
                self._trylock()
                return delay
            except error.LockHeld as inst:
                if self.ui and not debugged and 0 <= self.debugsecs <= delay:
                    self.ui.debug(inst.lockinfo.getwarning(self))
                    debugged = True

                if (
                    self.ui
                    and not warned
                    and self.warnsecs != -1
                    and 0 <= self.warnsecs <= delay
                ):
                    self.ui.warn(inst.lockinfo.getwarning(self))
                    warned = True

                if self.timeout >= 0 and delay >= self.timeout:
                    raise error.LockHeld(
                        errno.ETIMEDOUT, inst.filename, self.desc, inst.lockinfo
                    )

                if inst.lockinfo.pid == str(util.getpid()):
                    raise error.ProgrammingError(
                        "deadlock: %s was locked in the same process"
                        % self.vfs.join(self.f)
                    )

                time.sleep(0.1)
                delay += 0.1

    def _trylock(self):
        if self.held:
            self.held += 1
            return
        assert self._rustlock is None

        path = self.vfs.join(self.f)
        if (
            util.istest()
            and self.f
            in encoding.environ.get("EDENSCM_TEST_PRETEND_LOCKED", "").split()
        ):
            raise error.LockHeld(errno.EAGAIN, path, self.desc, None)

        try:
            if self.trylockfn:
                self._rustlock = self.trylockfn()
            else:
                self._rustlock = nativelock.pathlock.trylock(
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

    def _debugprintonce(self, msg):
        """Print debug message only once"""
        if not self.ui or msg in self._debugmessagesprinted:
            return
        self._debugmessagesprinted.add(msg)
        self.ui.debug(msg)

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
                    self._rustlock = None
                except OSError:
                    pass
            for callback in self.postrelease:
                callback()
            # Prevent double usage and help clear cycles.
            self.postrelease = None

    def _release(self):
        if self._rustlock:
            self._rustlock.unlock()


def islocked(vfs, name) -> bool:
    try:
        lock(vfs, name, timeout=0).release()
        return False
    except error.LockHeld:
        return True


def release(*locks) -> None:
    for lock in locks:
        if lock is not None:
            lock.release()
