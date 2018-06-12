#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import os

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
class SymlinkTest(EdenHgTestCase):
    def populate_backing_repo(self, repo):
        repo.write_file("contents1", "c1\n")
        repo.write_file("contents2", "c2\n")
        repo.symlink("symlink", "contents1")
        repo.commit("Initial commit.")

    def test_post_clone_permissions(self):
        st = os.lstat(os.path.join(self.mount, ".hg"))
        self.assertEqual(st.st_mode & 0o777, 0o755)
