# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

from eden.testlib.base import BaseTest, hgtest
from eden.testlib.repo import Repo
from eden.testlib.workingcopy import WorkingCopy


class TestBackoutHistory(BaseTest):
    def setUp(self) -> None:
        super().setUp()

    @hgtest
    def test_maintain_mv_data_non_p1(self, repo: Repo, wc: WorkingCopy) -> None:
        fileA = wc.file()
        wc.commit()
        fileB = wc.move(fileA)
        moved = wc.commit()

        # Make a random commit, so we trigger the backout case where we're not
        # backing out p1.
        wc.file()
        misc = wc.commit()

        for setting in ["on", "off"]:
            wc.checkout(misc)
            self.config.add("experimental", "copytrace", setting)

            backout = wc.backout(moved)
            self.assertEqual(backout.status().copies.get(fileA.path), fileB.path)
            repo.hide(backout)
