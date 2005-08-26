# lock.py - simple locking scheme for mercurial
#
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import os, time
import util

class LockHeld(Exception):
    pass

class lock:
    def __init__(self, file, wait=1):
        self.f = file
        self.held = 0
        self.wait = wait
        self.lock()

    def __del__(self):
        self.release()

    def lock(self):
        while 1:
            try:
                self.trylock()
                return 1
            except LockHeld, inst:
                if self.wait:
                    time.sleep(1)
                    continue
                raise inst

    def trylock(self):
        pid = os.getpid()
        try:
            util.makelock(str(pid), self.f)
            self.held = 1
        except (OSError, IOError):
            raise LockHeld(util.readlock(self.f))

    def release(self):
        if self.held:
            self.held = 0
            try:
                os.unlink(self.f)
            except: pass

