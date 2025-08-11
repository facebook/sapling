#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

from typing import Dict

from eden.fs.service.eden.thrift_types import GetScmStatusParams, ScmFileStatus

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

    async def _eden_status(
        self, listIgnored: bool = False
    ) -> Dict[bytes, ScmFileStatus]:
        async with self.eden.get_thrift_client() as client:
            status = await client.getScmStatusV2(
                GetScmStatusParams(
                    mountPoint=self.mount.encode(),
                    commit=self.initial_commit.encode(),
                    listIgnored=listIgnored,
                    rootIdOptions=None,
                )
            )
            return dict(status.status.entries)

    async def test_materialize(self) -> None:
        e = self.path + "/" + "e"
        self.write_file(e, "Small file in long path!\n")
        status = await self._eden_status()
        self.assertEqual(status, {e.encode(): ScmFileStatus.ADDED})
