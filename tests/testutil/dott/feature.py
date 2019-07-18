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


def _setuptesttmp():
    """Prepare the temporary directory. Return TESTTMP and HGRCPATH.

    This is for compatibility of auto-translated .t tests.
    New tests should use a different API that uses a context manager.
    """
    testtmp = os.environ.get("TESTTMP")
    hgrcpath = os.environ.get("HGRCPATH")
    if not (testtmp and hgrcpath):
        # Create new TESTTMP and HGRCPATH
        path = tempfile.mkdtemp(prefix="hgtest")
        if any(arg.startswith("--keep") for arg in sys.argv):
            atexit.register(sys.stderr.write, "Keeping tmpdir: %s\n" % path)
        else:
            atexit.register(shutil.rmtree, path, True)

        hgrcpath = os.path.join(path, ".hgrc")
        testtmp = os.path.join(path, "testtmp")
        shlib.mkdir(testtmp)
        os.chdir(testtmp)

    @normalizeoutput
    def replacetesttmp(out, testtmp=testtmp):
        return out.replace(testtmp, "$TESTTMP")

    # See _getenv from run-tests.py
    os.environ.update(
        {
            "COLUMNS": "80",
            "EMAIL": "Foo Bar <foo.bar@example.com>",
            "HGCOLORS": "16",
            "HGEDITOR": "true",
            "HGMERGE": "internal:merge",
            "HGRCPATH": hgrcpath,
            "HGUSER": "test",
            "HOME": testtmp,
            "LANG": "C",
            "LANGUAGE": "C",
            "LC_ALL": "C",
            "TESTTMP": testtmp,
            "TESTDIR": shlib.TESTDIR,
            "TZ": "GMT",
        }
    )
    open(hgrcpath, "w").write(
        """
[ui]
slash = True
interactive = False
mergemarkers = detailed
promptecho = True

[defaults]

[devel]
all-warnings = true
default-date = 0 0

[lfs]

[web]
address = localhost

[extensions]
treemanifest=

[treemanifest]
flatcompat=True

[remotefilelog]
reponame=reponame-default
cachepath=$TESTTMP/default-hgcache

"""
    )
    return testtmp, hgrcpath


TESTTMP, HGRCPATH = _setuptesttmp()


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
