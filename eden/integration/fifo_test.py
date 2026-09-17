#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import annotations

import errno
import os
import sys
import unittest

from .lib import testcase


@testcase.eden_repo_test
@unittest.skipUnless(sys.platform == "linux", "FIFOs are supported on Linux")
class FifoTest(testcase.EdenRepoTest):
    git_test_supported = False

    def populate_repo(self) -> None:
        self.repo.write_file("hello", "hola\n")
        self.repo.commit("Initial commit.")

    def test_fifo_io(self) -> None:
        path = os.path.join(self.mount, "pipe")
        # FIXME: mkfifo should succeed so builds can use named pipes in EdenFS.
        with self.assertRaises(OSError) as error:
            os.mkfifo(path, 0o600)
        self.assertEqual(errno.EPERM, error.exception.errno)
