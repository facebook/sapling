# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import print_function

import os
import random
import sys
import time


def waithook(ui, repo, **kwargs):
    """This hook is used to block pushes in some pushrebase tests

    It spins until `.hg/flag` exists
    """
    start = time.time()
    repo._wlockfreeprefix.add("hookrunning")
    repo.localvfs.writeutf8("hookrunning", "")
    while not repo.localvfs.exists("flag"):
        if time.time() - start > 20:
            print("ERROR: Timeout waiting for .hg/flag", file=sys.stderr)
            repo.localvfs.unlink("hookrunning")
            return True
        time.sleep(0.05)
    repo.localvfs.unlink("hookrunning")
    return False
