# lock.py - simple locking scheme for mercurial
#
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import errno, os, time
import util

class LockException(Exception):
    pass
class LockHeld(LockException):
    pass
class LockUnavailable(LockException):
    pass

class lock(object):
    def __init__(self, file, timeout=-1, releasefn=None):
        self.f = file
        self.held = 0
        self.timeout = timeout
        self.releasefn = releasefn
        self.lock()

    def __del__(self):
        self.release()

    def lock(self):
        timeout = self.timeout
        while 1:
            try:
                self.trylock()
                return 1
            except LockHeld, inst:
                if timeout != 0:
                    time.sleep(1)
                    if timeout > 0:
                        timeout -= 1
                    continue
                raise inst

    def trylock(self):
        pid = os.getpid()
        try:
            util.makelock(str(pid), self.f)
            self.held = 1
        except (OSError, IOError), why:
            if why.errno == errno.EEXIST:
                raise LockHeld(util.readlock(self.f))
            else:
                raise LockUnavailable(why)

    def release(self):
        if self.held:
            self.held = 0
            if self.releasefn:
                self.releasefn()
            try:
                os.unlink(self.f)
            except: pass

