#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

from typing import Optional

from facebook.eden.ttypes import (
    ChangeNotification,
    ChangesSinceV2Params,
    ChangesSinceV2Result,
    LargeChangeNotification,
    LostChangesReason,
    SmallChangeNotification,
)

from .lib import testcase


def getSmallChangeSafe(
    change: ChangeNotification,
) -> Optional[SmallChangeNotification]:
    if change.getType() == ChangeNotification.SMALLCHANGE:
        return change.get_smallChange()
    return None


def getLargeChangeSafe(
    change: ChangeNotification,
) -> Optional[LargeChangeNotification]:
    if change.getType() == ChangeNotification.LARGECHANGE:
        return change.get_largeChange()
    return None


@testcase.eden_repo_test
class ChangesTest(testcase.EdenRepoTest):
    def populate_repo(self) -> None:
        # Create the initial repo. It requires at least 1 file and 1 commit
        self.repo.write_file("hello", "bonjour\n")
        self.commit0 = self.repo.commit("Commit 0.")

    def setUp(self) -> None:
        # needs to be done before set up because these need to be created
        # for populate_repo() and the supers set up will call this.
        self.commit0 = ""
        self.commit1 = ""

        super().setUp()

        self.client = self.get_thrift_client_legacy()
        self.client.open()
        self.addCleanup(self.client.close)

        self.position = self.client.getCurrentJournalPosition(self.mount_path_bytes)

    def getChangesSinceV2(self, position) -> ChangesSinceV2Result:
        return self.client.changesSinceV2(
            ChangesSinceV2Params(
                mountPoint=self.mount_path_bytes, fromPosition=position
            )
        )

    def test_wrong_mount_generation(self):
        # The input mount generation should equal the current mount generation
        oldPosition = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        self.eden.unmount(self.mount_path)
        self.eden.mount(self.mount_path)
        changes = self.getChangesSinceV2(oldPosition)
        self.assertEqual(len(changes.changes), 1)
        largeChange = getLargeChangeSafe(changes.changes[0])
        self.assertIsNotNone(largeChange)
        self.assertEqual(
            largeChange.get_lostChanges().reason,
            LostChangesReason.EDENFS_REMOUNTED,
        )
