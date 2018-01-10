#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import os
import resource
import sys

from .lib import testcase


@testcase.eden_repo_test
class RestartTest:
    def populate_repo(self):
        self.pagesize = resource.getpagesize()
        self.page1 = "1" * self.pagesize
        self.page2 = "2" * self.pagesize
        self.repo.write_file('hello', self.page1 + self.page2)
        self.repo.commit('Initial commit.')

    def edenfs_logging_settings(self):
        return {'eden.strace': 'DBG7', 'eden.fs.fuse': 'DBG7'}

    def test_restart(self):
        hello = os.path.join(self.mount, 'hello')
        with open(hello, 'r') as f:
            # Read the first page only (rather than the whole file)
            # before we restart the process.
            # This is so that we can check that the kernel really
            # does call in to us for the second page and that we're
            # really servicing the read for the second page and that
            # it isn't just getting served from the kernel buffer cache
            self.assertEqual(self.page1, f.read(self.pagesize))

            print('=== beginning restart ===', file=sys.stderr)
            self.eden.graceful_restart()
            print('=== restart complete ===', file=sys.stderr)

            # Ensure that our file handle is still live across
            # the restart boundary
            f.seek(0)
            self.assertEqual(self.page1, f.read(self.pagesize))
            self.assertEqual(self.page2, f.read(self.pagesize))

        # Let's also testing opening the same file up again,
        # just to make sure that that is still working after
        # the graceful restart.
        with open(hello, 'r') as f:
            self.assertEqual(self.page1, f.read(self.pagesize))
            self.assertEqual(self.page2, f.read(self.pagesize))
