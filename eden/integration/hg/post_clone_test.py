#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import sys

from eden.integration.lib import hgrepo

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
# pyre-ignore[13]: T62487924
class SymlinkTest(EdenHgTestCase):
    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        repo.write_file("contents1", "c1\n")
        repo.write_file("contents2", "c2\n")
        repo.symlink("symlink", "contents1")
        repo.commit("Initial commit.")

    def test_post_clone_permissions(self) -> None:
        st = os.lstat(os.path.join(self.mount, ".hg"))
        expected_mode = 0o777 if sys.platform == "win32" else 0o755
        self.assertEqual(st.st_mode & 0o777, expected_mode)
