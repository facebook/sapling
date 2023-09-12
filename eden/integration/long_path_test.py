#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from typing import Dict

from facebook.eden.ttypes import GetScmStatusParams, ScmFileStatus

from .lib import testcase


@testcase.eden_repo_test()
class LongPathsTest(testcase.EdenRepoTest):
    """Verify that EdenFS behave properly when dealing with long paths."""

    path: str = "a" * 100 + "/" + "b" * 100 + "/" + "c" * 100
    file: str = path + "/" + "d" * 100
    initial_commit: str = ""

    def populate_repo(self) -> None:
        self.repo.write_file(self.file, "Long path!\n")
        self.initial_commit = self.repo.commit("a")

    def test_read(self) -> None:
        self.assertEqual(self.read_file(self.file), "Long path!\n")

    def _eden_status(self, listIgnored: bool = False) -> Dict[bytes, ScmFileStatus]:
        with self.eden.get_thrift_client_legacy() as client:
            status = client.getScmStatusV2(
                GetScmStatusParams(
                    mountPoint=self.mount.encode(),
                    commit=self.initial_commit.encode(),
                    listIgnored=listIgnored,
                )
            )
            return status.status.entries

    def test_materialize(self) -> None:
        e = self.path + "/" + "e"
        self.write_file(e, "Small file in long path!\n")
        self.assertEqual(self._eden_status(), {e.encode(): ScmFileStatus.ADDED})
