#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

from eden.fs.service.eden.types import (
    JournalPosition as JournalPosition_py3,
    ScmFileStatus,
)
from eden.fs.service.streamingeden.types import StreamChangesSinceParams
from eden.integration.lib import hgrepo
from facebook.eden.ttypes import JournalPosition as JournalPosition_py
from thrift.py3.converter import to_py3_struct
from thrift.util.converter import to_py_struct

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
# pyre-ignore[13]: T62487924
class EdenJournalTest(EdenHgTestCase):
    commit1: str
    commit2: str

    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        repo.write_file("hello.txt", "hello\n")
        self.commit1 = repo.commit("Initial commit")
        repo.write_file("foo/bar.txt", "bar\n")
        self.commit2 = repo.commit("Commit 2")

    def test_journal_position_write(self) -> None:
        """
        Verify that the journal is updated when writing to the working copy.
        """
        with self.get_thrift_client_legacy() as client:
            before = client.getCurrentJournalPosition(self.mount_path_bytes)

        self.repo.write_file("hello.txt", "hola\n")

        with self.get_thrift_client_legacy() as client:
            after = client.getCurrentJournalPosition(self.mount_path_bytes)

        self.assertNotEqual(before, after)

    async def test_journal_stream_changes_since(self) -> None:
        """
        Verify that the streamChangesSince API reports all the changed
        files/directories across update.
        """

        with self.get_thrift_client_legacy() as client:
            before = client.getCurrentJournalPosition(self.mount_path_bytes)

        self.repo.update(self.commit1)

        self.repo.write_file("hello.txt", "hola\n")
        self.repo.write_file("bar.txt", "bar\n")

        added = set()
        removed = set()
        modified = set()

        async with self.get_thrift_client() as client:
            params = StreamChangesSinceParams(
                mountPoint=self.mount_path_bytes,
                fromPosition=to_py3_struct(JournalPosition_py3, before),
            )
            result, changes = await client.streamChangesSince(params)
            async for change in changes:
                path = change.name.decode()
                if path.startswith(".hg"):
                    continue

                status = change.status
                if status == ScmFileStatus.ADDED:
                    added.add(path)
                elif status == ScmFileStatus.MODIFIED:
                    modified.add(path)
                else:
                    self.assertEqual(status, ScmFileStatus.REMOVED)
                    removed.add(path)

        # Files not in commits:
        self.assertIn("hello.txt", modified)
        self.assertIn("bar.txt", added)

        # Files in commits:
        self.assertIn("foo/bar.txt", removed)

        # The directory is also removed.
        self.assertIn("foo", removed)

        self.assertNotEqual(before, to_py_struct(JournalPosition_py, result.toPosition))

        counter_name = (
            "thrift.StreamingEdenService.streamChangesSince.streaming_time_us.avg.60"
        )
        counters = self.get_counters()
        self.assertIn(counter_name, counters)
        self.assertGreater(counters[counter_name], 0)
