#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os

from eden.integration.lib import hgrepo

from .hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
# pyre-fixme[38]: `HgExtensionTestBaseTest` does not implement all inherited
#  abstract methods.
# pyre-fixme[13]: Attribute `backing_repo` is never initialized.
# pyre-fixme[13]: Attribute `backing_repo_name` is never initialized.
# pyre-fixme[13]: Attribute `config_variant_name` is never initialized.
# pyre-fixme[13]: Attribute `repo` is never initialized.
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
