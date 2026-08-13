# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""I/O errors raised by Rust are catchable as the matching OSError subclass.

Rust reports I/O errors as `OSError` plus its constructor arguments, and
CPython only builds the concrete exception when the error is normalized.
Normalization keeps `OSError` as the exception type even though
`OSError.__new__` picks a subclass from errno, so on CPython 3.10 and older -
which matches `except` clauses against the type rather than the instance -
`except FileNotFoundError` did not catch a missing file reported by Rust.
`localrepo.transaction()` relies on exactly that, which used to make every
locked transaction (clone, pull, rebase) abort on Windows builds, where the
embedded interpreter is CPython 3.10.

CPython 3.12 and newer normalize exceptions when they are raised, so these
tests pass with or without the fix there. The check that is meaningful on
every Python version lives in the Rust unit tests of
eden/scm/lib/cpython-ext/src/io_error.rs.
"""

import os
import tempfile
import unittest

import silenttestrunner
from sapling import vfs as vfsmod


class testrustioerrors(unittest.TestCase):
    def setUp(self):
        self.vfs = vfsmod.vfs(tempfile.mkdtemp(dir=os.getcwd()), audit=False)

    def teststat(self):
        with self.assertRaises(FileNotFoundError):
            self.vfs.stat("missing")

    def testlstat(self):
        with self.assertRaises(FileNotFoundError):
            self.vfs.lstat("missing")

    def testread(self):
        with self.assertRaises(FileNotFoundError):
            self.vfs.read("missing")

    def testlistdir(self):
        with self.assertRaises(FileNotFoundError):
            self.vfs.listdir("missing")

    def testunlink(self):
        with self.assertRaises(FileNotFoundError):
            self.vfs.unlink("missing")

    def testlexistsofmissingvfs(self):
        # lexists() swallows FileNotFoundError to report a vfs whose own base
        # directory is gone.
        missing = vfsmod.vfs(os.path.join(self.vfs.base, "missing"), audit=False)
        self.assertFalse(missing.lexists("anything"))


if __name__ == "__main__":
    silenttestrunner.main(__name__)
