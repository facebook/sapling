#!/usr/bin/env python3
#
# Copyright (c) 2004-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import configparser
import os
from typing import List, Optional

from eden.integration.hg.lib.hg_extension_test_base import (
    EdenHgTestCase,
    get_default_hgrc,
)
from eden.integration.lib import hgrepo


class FlatmanifestFallbackUpdateTest(EdenHgTestCase):
    commit1: str
    commit2: str
    commit3: str
    commit4: str

    def apply_hg_config_variant(self, hgrc: configparser.ConfigParser) -> None:
        # Do nothing here for now.
        # Keep treemanifest disabled initially during populate_backing_repo()
        pass

    def edenfs_extra_args(self) -> Optional[List[str]]:
        # Explicitly allow eden to fallback to flatmanifest import
        # if it fails to import treemanifest data, since that is what
        # we are trying to test.
        return ["--allow_flatmanifest_fallback=yes"]

    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        # Create a couple commits in flatmanifest mode
        repo.write_file("src/main.sh", "echo hello world\n")
        repo.write_file("src/test/test.sh", "echo success\n")
        repo.write_file("src/.gitignore", "*.o\n")
        repo.write_file("doc/readme.txt", "sample repository\n")
        repo.write_file(".gitignore", "ignoreme\n")
        self.commit1 = repo.commit("Initial commit.")

        repo.write_file("src/.gitignore", "*.[oa]\n")
        self.commit2 = repo.commit("Update src/.gitignore")

        # Now enable treemanifest
        # Note that we don't set paths.default or remotefilelog.fallbackpath
        # here, so treemanifest prefetching will always fail since it does not
        # have a remote repository to fetch from.
        hgrc = get_default_hgrc()
        hgrc["extensions"]["fastmanifest"] = ""
        hgrc["extensions"]["treemanifest"] = ""
        hgrc["fastmanifest"] = {"usetree": "True", "usecache": "False"}
        hgrc["remotefilelog"] = {
            "reponame": "eden_integration_tests",
            "cachepath": os.path.join(self.tmp_dir, "hgcache"),
        }
        repo.write_hgrc(hgrc)

        # Create a couple new commits now that treemanifest is enabled
        repo.write_file("src/test/test2.sh", "echo success2\n")
        self.commit3 = repo.commit("Add test2")
        repo.write_file("src/test/test2.sh", "echo success\necho success\n")
        self.commit4 = repo.commit("Update test2")

    def test_checkout_flatmanifest(self) -> None:
        # Check our status
        self.assertEqual(self.commit4, self.repo.get_head_hash())
        self.assert_status_empty()
        self.assertTrue(os.path.exists(self.get_path("src/test/test2.sh")))
        self.assertEqual(
            "echo success\necho success\n", self.read_file("src/test/test2.sh")
        )
        self.repo.update(self.commit2)

        # Now try checking out self.commit2
        # This commit was created without treemanifest data, so the import
        # process will need to fall back to a flatmanifest import even though
        # treemanifest is enabled in this repository.
        self.assertEqual(self.commit2, self.repo.get_head_hash())
        self.assert_status_empty()
        self.assertFalse(os.path.exists(self.get_path("src/test/test2.sh")))
