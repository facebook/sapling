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
    repo.localvfs.write("hookrunning", "")
    while not repo.localvfs.exists("flag"):
        if time.time() - start > 20:
            print >>sys.stderr, "ERROR: Timeout waiting for .hg/flag"
            repo.localvfs.unlink("hookrunning")
            return True
        time.sleep(0.05)
    repo.localvfs.unlink("hookrunning")
    return False
