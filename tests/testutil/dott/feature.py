# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""hghave and testcase support for .t tests.

Support `#require`, `#if`, `#testcases` used by .t tests.
"""

from __future__ import absolute_import

import atexit
import inspect
import os
import shutil
import sys
import tempfile

import hghave

from . import shlib
from .shobj import normalizeoutput


def check(name):
    """Return True if name passes hghave check"""
    if isinstance(name, list):
        return all(check(n) for n in name)
    else:
        # Find the "testcase" name in the stack
        testcase = None
        frame = inspect.currentframe().f_back
        while testcase is None and frame is not None:
            testcase = frame.f_locals.get("testcase")
            frame = frame.f_back
        frame = None
        if testcase == name:
            return True
        elif "no-%s" % testcase == name:
            return False
        # Check using hghave
        res = hghave.checkfeatures([name])
        return not (res["error"] or res["missing"] or res["skipped"])


def require(name):
    """Exit the test if the required feature is not available"""
    if not check(name):
        sys.stderr.write("skipped: missing feature: %r\n" % name)
        sys.exit(80)
