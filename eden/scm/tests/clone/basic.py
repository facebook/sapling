# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

from eden.testlib.base import BaseTest, hgtest
from eden.testlib.repo import Repo
from eden.testlib.util import new_dir
from eden.testlib.workingcopy import WorkingCopy


class TestBasicClone(BaseTest):
    def setUp(self) -> None:
        super().setUp()
        self.config.add("clone", "force-rust", "True")

    @hgtest
    def test_repo_specific_config(self, repo: Repo, _: WorkingCopy) -> None:
        config_dir = new_dir()
        config_dir.joinpath(f"{repo.name}.rc").write_text("[foo]\nbar=baz\n")

        self.config.add("clone", "repo-specific-config-dir", str(config_dir))

        cloned_repo = self.server.clone()

        self.assertEqual(
            cloned_repo.hg.config("foo.bar").stdout.strip(),
            "baz",
        )
