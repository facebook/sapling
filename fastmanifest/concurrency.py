# concurrency.py
#
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import errno
import resource
import os
import socket
import stat
import sys
import time

from mercurial import error

# Returns true if we're the original process, returns false if we're the child
# process.
#
# NOTE: This is extremely platform-specific code.
def fork_worker(ui, repo, silent_worker):
    if not silent_worker:
        # if we don't want a silent worker, then we need to flush any streams so
        # any buffered content only gets written *once*.
        sys.stdout.flush()
        sys.stderr.flush()

    pid = os.fork()
    if pid > 0:
        return True

    if silent_worker:
        # close all file descriptors.
        flimit = resource.getrlimit(resource.RLIMIT_NOFILE)
        os.closerange(0, flimit[0])

        # reopen some new file handles.
        ui.fin = sys.stdin = open(os.devnull, "r")
        ui.fout = ui.ferr = sys.stdout = sys.stderr = open(os.devnull, "w")
        repo.ui = ui

    os.setsid()
    pid = os.fork()
    if pid > 0:
        os._exit(0)

    return False

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
                raise error.LockHeld(ex.errno,
                                     self.vfs.join(self.lockname),
                                     self.lockname,
                                     "unimplemented")
            raise error.LockUnavailable(ex.errno,
                                        ex.strerror,
                                        self.vfs.join(self.lockname),
                                        self.lockname)

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
        lockcontents = '%s:%s' % (looselock._host, os.getpid())

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
        return (self.stealcount != 0 or
                self.refcount != 0)

    def __enter__(self):
        return self.lock()

    def __exit__(self, exc_type, exc_value, exc_tb):
        return self.unlock()
