# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

from eden.testlib.base import BaseTest, hgtest
from eden.testlib.hg import CommandFailure
from eden.testlib.repo import Repo
from eden.testlib.util import new_dir
from eden.testlib.workingcopy import WorkingCopy


class TestResumeClone(BaseTest):
    def setUp(self) -> None:
        super().setUp()

        self.config.add("remotenames", "selectivepulldefault", "master")
        self.config.add("commands", "force-rust", "clone")
        self.config.add("experimental", "nativecheckout", "True")

    @hgtest
    def test_resume(self, repo: Repo, wc: WorkingCopy) -> None:
        wc.file(path="foo")
        commit1 = wc.commit()

        wc.hg.push(rev=commit1.hash, to="master", create=True)

        clone_wc = WorkingCopy(repo, new_dir())

        with self.assertRaises(CommandFailure) as cm:
            repo.hg.clone(
                repo.url,
                clone_wc.root,
                env={"FAILPOINTS": "checkout-post-progress=return"},
            )

        self.assertEqual(len(clone_wc.status().untracked), 1)
        self.assertIn("hg checkout --continue", cm.exception.result.stdout)

        # Make sure "checkout --continue" works and skips the file.
        self.assertRegex(
            clone_wc.hg.checkout(
                **{"continue": True, "env": {"EDENSCM_LOG": "checkout=debug"}}
            ).stderr,
            "Skipping checking out 1 files since they're already written",
        )
        self.assertTrue(clone_wc.status().empty())
