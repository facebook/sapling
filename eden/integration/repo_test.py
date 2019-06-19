#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
from typing import List

from .lib import testcase


class RepoTest(testcase.EdenTestCase):
    """
    Tests for the "eden repository" command.

    Note that these tests do not use @testcase.eden_repo_test, since we don't
    actually need to run separately with git and mercurial repositories.  These
    tests don't actually mount anything in eden at all.
    """

    def test_list_repository(self) -> None:
        self.assertEqual([], self._list_repos())

        config = """\
["repository fbsource"]
path = "/data/users/carenthomas/fbsource"
type = "git"

["bindmounts fbsource"]
fbcode-buck-out = "fbcode/buck-out"
fbandroid-buck-out = "fbandroid/buck-out"
fbobjc-buck-out = "fbobjc/buck-out"
buck-out = "buck-out"

["repository git"]
path = "/home/carenthomas/src/git"
type = "git"

["repository hg-crew"]
url = "/data/users/carenthomas/facebook-hg-rpms/hg-crew"
type = "hg"
"""
        home_config_file = os.path.join(self.home_dir, ".edenrc")
        with open(home_config_file, "w") as f:
            f.write(config)

        expected = ["fbsource", "git", "hg-crew"]
        self.assertEqual(expected, self._list_repos())

    def test_add_multiple(self) -> None:
        hg_repo = self.create_hg_repo("hg_repo")
        git_repo = self.create_git_repo("git_repo")

        self.eden.add_repository("hg1", hg_repo.path)
        self.assertEqual(["hg1"], self._list_repos())
        self.eden.add_repository("hg2", hg_repo.path)
        self.assertEqual(["hg1", "hg2"], self._list_repos())
        self.eden.add_repository("git1", git_repo.path)
        self.assertEqual(["git1", "hg1", "hg2"], self._list_repos())

    def _list_repos(self) -> List[str]:
        return self.eden.repository_cmd().splitlines()
