# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import os

from .base import BaseTest, hgtest
from .repo import Repo
from .workingcopy import WorkingCopy


class TestLibTests(BaseTest):
    @hgtest
    def test_repo_setup(self, repo: Repo, wc: WorkingCopy) -> None:
        self.assertEqual(repo.root, wc.root)
        self.assertTrue(os.path.exists(os.path.join(repo.root, ".hg")))


if __name__ == "__main__":
    import unittest

    unittest.main()
