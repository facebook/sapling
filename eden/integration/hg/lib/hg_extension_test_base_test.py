#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import os

from eden.integration.lib import hgrepo

from .hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
class HgExtensionTestBaseTest(EdenHgTestCase):
    """Test to make sure that HgExtensionTestBase creates Eden mounts that are
    properly configured with the Hg extension.
    """

    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        repo.write_file("hello.txt", "hola")
        repo.commit("Initial commit.")

    def test_setup(self) -> None:
        hg_dir = os.path.join(self.mount, ".hg")
        self.assertTrue(os.path.isdir(hg_dir))

        eden_extension = self.hg("config", "extensions.eden").rstrip()
        self.assertEqual("", eden_extension)

        self.assertTrue(os.path.isfile(self.get_path("hello.txt")))
