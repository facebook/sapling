#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import subprocess
import sys
import time
from typing import Dict, List, Optional

from facebook.eden.ttypes import (
    ChangeNotification,
    ChangesSinceV2Params,
    ChangesSinceV2Result,
    Dtype,
    SmallChangeNotification,
    SynchronizeWorkingCopyParams,
)

from . import testcase
from .thrift_objects import buildSmallChange


class JournalTestBase(testcase.EdenRepoTest):
    git_test_supported = False

    def edenfs_extra_config(self) -> Optional[Dict[str, List[str]]]:
        configs = super().edenfs_extra_config()
        if configs:
            configs["notify"] = ['max-num-changes = "100"']
        return configs

    def edenfs_logging_settings(self) -> Dict[str, str]:
        return {"eden": "DBG3"}

    def populate_repo(self) -> None:
        # Create the initial repo. It requires at least 1 file and 1 commit
        self.repo.write_file("hello", "bonjour\n")
        self.repo.write_file("the_land/is", "cloaked\n")
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

    def check_changes(self, changes, expected_changes) -> bool:
        expected_changes_index = 0
        for change in changes:
            if change == expected_changes[expected_changes_index]:
                expected_changes_index += 1
                if expected_changes_index == len(expected_changes):
                    return True
        print("Expected changes not found:")
        for i in range(expected_changes_index, len(expected_changes)):
            print(expected_changes[i])
        print("in:")
        print(changes)
        return False

    def getChangesSinceV2(
        self,
        position,
        included_roots=None,
        excluded_roots=None,
        included_suffixes=None,
        excluded_suffixes=None,
        root=None,
    ) -> ChangesSinceV2Result:
        if sys.platform == "win32":
            # On Windows, we need to wait for the file system to settle before
            # calling getChangesSinceV2. Otherwise, we may get missing results.
            self.client.synchronizeWorkingCopy(
                self.mount_path_bytes, SynchronizeWorkingCopyParams()
            )
        return self.client.changesSinceV2(
            ChangesSinceV2Params(
                mountPoint=self.mount_path_bytes,
                fromPosition=position,
                includedRoots=included_roots,
                excludedRoots=excluded_roots,
                includedSuffixes=included_suffixes,
                excludedSuffixes=excluded_suffixes,
                root=root,
            )
        )

    def repo_write_file(self, path, contents, mode=None, add=True) -> None:
        self.eden_repo.write_file(path, contents, mode, add)

    def setup_test_add_file(self) -> ChangesSinceV2Result:
        position = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        self.repo_write_file("test_file", "", add=False)
        return self.getChangesSinceV2(position=position)

    def setup_test_add_file_root(self, root) -> ChangesSinceV2Result:
        position = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        self.repo_write_file(f"{root}/test_file", "", add=False)
        return self.getChangesSinceV2(position=position, root=root)

    def setup_test_rename_file(self) -> ChangesSinceV2Result:
        self.repo_write_file("test_file", "", add=False)
        position = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        self.rename("test_file", "best_file")
        return self.getChangesSinceV2(position=position)

    def repo_rmdir(self, path) -> None:
        self.rmdir(path)

    def add_file_expect(
        self, path, contents, mode=None, add=True
    ) -> List[ChangeNotification]:
        self.repo_write_file(path, contents, mode, add)
        return [
            buildSmallChange(
                SmallChangeNotification.ADDED, Dtype.REGULAR, path=path.encode()
            ),
            buildSmallChange(
                SmallChangeNotification.MODIFIED, Dtype.REGULAR, path=path.encode()
            ),
        ]

    def add_folder_expect(self, path) -> List[ChangeNotification]:
        self.mkdir(path)
        return [
            buildSmallChange(
                SmallChangeNotification.ADDED, Dtype.DIR, path=path.encode()
            ),
        ]

    def repo_chmod(self, fd, mode) -> None:
        self.chmod(fd, mode)

    def repo_chown(self, fd) -> None:
        # because chown needs sudo to change to nobody
        fullpath = self.eden_repo.get_path(fd)
        cmd = ["sudo", "chown", "nobody:nobody", fullpath]
        subprocess.call(cmd)


class WindowsJournalTestBase(JournalTestBase):
    # This class is intended to test the journal system for EdenFS on Windows.
    # This is required because file changes are not immediately reported to Eden,
    # so we need to wait for the journal to update before checking its status
    SYNC_MAX: int = 1  # noqa

    def syncProjFS(self, position) -> None:
        # Wait for eden to get the PrjFS notification
        pollTime = 0.1
        waitTime = 0
        newPosition = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        while position == newPosition and waitTime < self.SYNC_MAX:
            time.sleep(pollTime)
            waitTime += pollTime
            newPosition = self.client.getCurrentJournalPosition(self.mount_path_bytes)

    def repo_write_file(self, path, contents, mode=None, add=True) -> None:
        position = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        super().repo_write_file(path, contents, mode, add)
        self.syncProjFS(position)

    def rm(self, path) -> None:
        position = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        super().rm(path)
        self.syncProjFS(position)

    def rename(self, from_path, to_path) -> None:
        position = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        super().rename(from_path, to_path)
        self.syncProjFS(position)

    def mkdir(self, path) -> None:
        position = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        super().mkdir(path)
        self.syncProjFS(position)

    def repo_rmdir(self, path) -> None:
        position = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        super().rmdir(path)
        self.syncProjFS(position)

    def repo_chmod(self, fd, mode) -> None:
        position = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        self.chmod(fd, mode)
        self.syncProjFS(position)

    def repo_chown(self, fd) -> None:
        position = self.client.getCurrentJournalPosition(self.mount_path_bytes)
        super().repo_chown(fd)
        self.syncProjFS(position)

    def add_file_expect(
        self, path, contents, mode=None, add=True
    ) -> List[ChangeNotification]:
        self.repo_write_file(path, contents, mode, add)
        return [
            buildSmallChange(
                SmallChangeNotification.ADDED, Dtype.REGULAR, path=path.encode()
            ),
        ]
