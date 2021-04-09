# Test extension that adds delays before or after acquiring a lock.
#
# This extension can be used to test race conditions between lock acquisition.
#
# The delays are controlled by environment variables.  The ``DELAY``
# environment variable sets a flat delay in seconds.  The ``FILE``
# environment variable sets a filename, and the lock is delays until
# that file is removed.

from __future__ import absolute_import

import os
import time


def delay(key):
    delay = float(os.environ.get("HG%sDELAY" % key, "0.0"))
    if delay:
        time.sleep(delay)
    filename = os.environ.get("HG%sFILE" % key)
    if filename:
        while os.path.exists(filename):
            time.sleep(0.1)


def reposetup(ui, repo):
    class delayedlockrepo(repo.__class__):
        def lock(self, wait=True):
            delay("PRELOCK")
            res = super(delayedlockrepo, self).lock(wait)
            delay("POSTLOCK")
            return res

        def wlock(self, wait=True):
            delay("PREWLOCK")
            res = super(delayedlockrepo, self).wlock(wait)
            delay("POSTWLOCK")
            return res

    repo.__class__ = delayedlockrepo
