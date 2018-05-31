#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import os

from .lib import testcase


class StartTest(testcase.EdenTestCase):
    def test_start_if_necessary(self) -> None:
        # Confirm there are no checkouts configured, then stop edenfs
        checkouts = self.eden.list_cmd()
        self.assertEqual({}, checkouts)
        self.assertTrue(self.eden.is_healthy())
        self.eden.shutdown()
        self.assertFalse(self.eden.is_healthy())

        # `eden start --if-necessary` should not start eden
        output = self.eden.run_cmd("start", "--if-necessary")
        self.assertEqual("No Eden mount points configured.\n", output)
        self.assertFalse(self.eden.is_healthy())

        # Restart eden and create a checkout
        self.eden.start()
        self.assertTrue(self.eden.is_healthy())

        # Create a repository with one commit
        repo = self.create_hg_repo("testrepo")
        repo.write_file("README", "test\n")
        repo.commit("Initial commit.")
        # Create an Eden checkout of this repository
        checkout_dir = os.path.join(self.mounts_dir, "test_checkout")
        self.eden.clone(repo.path, checkout_dir)

        checkouts = self.eden.list_cmd()
        self.assertEqual({checkout_dir: self.eden.CLIENT_ACTIVE}, checkouts)

        # Stop edenfs
        self.eden.shutdown()
        self.assertFalse(self.eden.is_healthy())
        # `eden start --if-necessary` should start edenfs now
        # TODO: We unfortunately need to specify capture_output=False at the moment,
        # since `eden start` daemonizes, but it's stdout doesn't actually get fully
        # closed since it is being held open by sudo.  This capture_output setting
        # can be removed once we move the daemonization logic from python into the
        # edenfs C++ binary.
        if "SANDCASTLE" in os.environ:
            self.eden.run_cmd(
                "start", "--if-necessary", "--", "--allowRoot", capture_output=False
            )
        else:
            self.eden.run_cmd("start", "--if-necessary", capture_output=False)
        # self.assertIn("Started edenfs", output)
        self.assertTrue(self.eden.is_healthy())

        # Stop edenfs.  We didn't start it through self.eden.start()
        # so the self.eden class doesn't really know it is running and that
        # it needs to be shut down.
        self.eden.run_cmd("stop")
