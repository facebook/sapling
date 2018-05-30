# concurrency.py
#
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
from __future__ import absolute_import

import errno
import os
import socket
import stat
import subprocess
import sys
import time
import traceback

from mercurial import error, pycompat


class looselock(object):
    """A loose lock.  If the lock is held and the lockfile is recent, then we
    immediately fail.  If the lockfile is older than X seconds, where
    X=stealtime, then we touch the lockfile and proceed.  This is slightly
    vulnerable to a thundering herd, as a bunch of callers that arrive at the
    expiration may all proceed."""

    _host = None

    def __init__(self, vfs, lockname, stealtime=10.0):
        self.vfs = vfs
        self.lockname = lockname
        self.stealtime = stealtime

        self.refcount = 0
        self.stealcount = 0

    def _trylock(self, lockcontents):
        """Attempt to acquire a lock.

        Raise error.LockHeld if the lock is already held.

        Raises error.LockUnavailable if the lock could not be acquired for any
        other reason.

        This is an internal API, and shouldn't be called externally.

        """
        try:
            self.vfs.makelock(lockcontents, self.lockname)
        except (OSError, IOError) as ex:
            if ex.errno == errno.EEXIST:
                raise error.LockHeld(
                    ex.errno,
                    self.vfs.join(self.lockname),
                    self.lockname,
                    "unimplemented",
                )
            raise error.LockUnavailable(
                ex.errno, ex.strerror, self.vfs.join(self.lockname), self.lockname
            )

    def lock(self):
        """Attempt to acquire a lock.

        Raise error.LockHeld if the lock is already held and the lock is too
        recent to be stolen.

        Raises error.LockUnavailable if the lock could not be acquired for any
        other reason.
        """
        if self.stealcount > 0:
            # we stole the lock, so we should continue stealing.
            self.stealcount += 1
            return self

        if looselock._host is None:
            looselock._host = socket.gethostname()
        lockcontents = "%s:%s" % (looselock._host, os.getpid())

        try:
            self._trylock(lockcontents)
        except error.LockHeld:
            # how old is the file?
            steal = False
            try:
                fstat = self.vfs.lstat(self.lockname)
                mtime = fstat[stat.ST_MTIME]
                if time.time() - mtime > self.stealtime:
                    # touch the file
                    self.vfs.utime(self.lockname)

                    steal = True
                else:
                    raise
            except OSError as ex:
                if ex.errno == errno.ENOENT:
                    steal = True
                else:
                    raise

            if steal:
                # we shouldn't have any hard references
                assert self.refcount == 0

                # bump the stealcount
                self.stealcount += 1
        else:
            self.refcount += 1

        return self

    def unlock(self):
        """Releases a lock."""
        if self.stealcount > 1:
            self.stealcount -= 1
            return
        elif self.refcount > 1:
            self.refcount -= 1
            return
        elif self.refcount == 1 or self.stealcount == 1:
            # delete the file
            try:
                self.vfs.unlink(self.lockname)
            except OSError as ex:
                if ex.errno == errno.ENOENT:
                    pass
                else:
                    raise

            self.refcount = 0
            self.stealcount = 0

    def held(self):
        return self.stealcount != 0 or self.refcount != 0

    def __enter__(self):
        return self.lock()

    def __exit__(self, exc_type, exc_value, exc_tb):
        return self.unlock()


# This originated in hgext/logtoprocess.py, was copied to
# remotefilelog/shallowutil.py, and now here.
if pycompat.iswindows:
    # no fork on Windows, but we can create a detached process
    # https://msdn.microsoft.com/en-us/library/windows/desktop/ms684863.aspx
    # No stdlib constant exists for this value
    DETACHED_PROCESS = 0x00000008
    _creationflags = DETACHED_PROCESS | subprocess.CREATE_NEW_PROCESS_GROUP

    def runshellcommand(script, env=None, silent_worker=True):
        if not silent_worker:
            raise NotImplementedError("support for non-silent workers not yet built.")

        # we can't use close_fds *and* redirect stdin. I'm not sure that we
        # need to because the detached process has no console connection.
        subprocess.Popen(script, env=env, close_fds=True, creationflags=_creationflags)


else:

    def runshellcommand(script, env=None, silent_worker=True):
        # double-fork to completely detach from the parent process
        # based on http://code.activestate.com/recipes/278731
        pid = os.fork()
        if pid:
            # parent
            return
        # subprocess.Popen() forks again, all we need to add is
        # flag the new process as a new session.
        newsession = {}
        if silent_worker:
            if sys.version_info < (3, 2):
                newsession["preexec_fn"] = os.setsid
            else:
                newsession["start_new_session"] = True
        try:
            # connect stdin to devnull to make sure the subprocess can't
            # muck up that stream for mercurial.
            if silent_worker:
                stderr = stdout = open(os.devnull, "w")
            else:
                stderr = stdout = None
            subprocess.Popen(
                script,
                stdout=stdout,
                stderr=stderr,
                stdin=open(os.devnull, "r"),
                env=env,
                close_fds=True,
                **newsession
            )
        except Exception:
            if not silent_worker:
                sys.stderr.write("Error spawning worker\n")
                traceback.print_exc(file=sys.stderr)
        finally:
            # mission accomplished, this child needs to exit and not
            # continue the hg process here.

            if not silent_worker:
                sys.stdout.flush()
                sys.stderr.flush()
            os._exit(0)
