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
        self.config.add("commands", "force-rust", "True")

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

    @hgtest
    def test_dirstate_mtime_race(self, repo: Repo, wc: WorkingCopy) -> None:
        wc.file(path="foo", content="foo")
        commit1 = wc.commit()

        wc.hg.push(rev=commit1.hash, to="master", create=True)

        other_wc = WorkingCopy(repo, new_dir())
        other_wc.hg.clone(repo.url, other_wc.root)

        # Try to update our file in the same second the checkout finished.
        other_wc["foo"].write("bar")

        # Make sure we pick up "foo" as modified.
        self.assertEqual(
            other_wc.status().modified,
            ["foo"],
        )
