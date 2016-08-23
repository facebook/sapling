# Dummy extension that adds a delay after acquiring a lock.
#
# This extension can be used to test race conditions between lock acquisition.

from __future__ import absolute_import

import os
import time

def reposetup(ui, repo):

    class delayedlockrepo(repo.__class__):
        def lock(self):
            delay = float(os.environ.get('HGPRELOCKDELAY', '0.0'))
            if delay:
                time.sleep(delay)
            res = super(delayedlockrepo, self).lock()
            delay = float(os.environ.get('HGPOSTLOCKDELAY', '0.0'))
            if delay:
                time.sleep(delay)
            return res
    repo.__class__ = delayedlockrepo
