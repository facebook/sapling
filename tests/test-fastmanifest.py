import silenttestrunner
import unittest
import os
import sys

from mercurial import manifest
from mercurial import util

class HybridManifest(unittest.TestCase):

  def test_wrap(self):
    """If a new magic method is added to manifestdict, we want to make sure that
       hybridmanifest implement it, this test validates that all magic methods
       of manifestdict are implemented by hybridmanifest to avoid breakage in
       prod"""
    hd = fastmanifest.hybridmanifest(None)
    ismagic = lambda x: x.startswith("__") and x.endswith("__")
    magicmethods = [k for k, v in  manifest.manifestdict.__dict__.items()
                      if util.safehasattr(v, '__call__') and ismagic(k)]
    for method in magicmethods:
        assert util.safehasattr(hd, method),\
            "%s missing in hybrid manifest" % method

if __name__ == "__main__":
    sys.path.insert(0, os.path.join(os.environ["TESTDIR"], ".."))
    import fastmanifest
    silenttestrunner.main(__name__)
