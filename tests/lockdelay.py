# Dummy extension that adds a delay after acquiring a lock.
#
# This extension can be used to test race conditions between lock acquisition.

from __future__ import absolute_import

import os
import time

from mercurial import (
    lock as lockmod,
)

class delaylock(lockmod.lock):
    def lock(self):
        delay = float(os.environ.get('HGPRELOCKDELAY', '0.0'))
        if delay:
            time.sleep(delay)
        res = super(delaylock, self).lock()
        delay = float(os.environ.get('HGPOSTLOCKDELAY', '0.0'))
        if delay:
            time.sleep(delay)
        return res

def extsetup(ui):
    lockmod.lock = delaylock
