#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import os
from typing import Optional

from eden.fs.service.eden.thrift_types import ScmFileStatus
from eden.fs.service.streamingeden.thrift_clients import StreamingEdenService
from eden.fs.service.streamingeden.thrift_types import (
    StreamChangesSinceParams,
    StreamSelectedChangesSinceParams,
)
from eden.integration.lib import hgrepo
from thrift.python.client import ClientType, get_client

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

    def get_streaming_client(
        self, timeout: Optional[float] = None
    ) -> StreamingEdenService.Async:
        eden_dir = self.eden._eden_dir
        socket_path = os.path.join(eden_dir, "socket")
        if timeout is None:
            timeout = 0
        return get_client(
            StreamingEdenService,
            path=socket_path,
            timeout=timeout,
            client_type=ClientType.THRIFT_ROCKET_CLIENT_TYPE,
        )

    async def test_journal_stream_selected_changes_since_complex_globs(self) -> None:
        """
        Verify that the streamSelectedChangesSince API reports only the
        selected files/directories across update.
        """
        async with self.get_streaming_client() as client:
            before = await client.getCurrentJournalPosition(self.mount_path_bytes)
        self.repo.write_file("b.cpp", "//comment\n")
        self.repo.write_file("bar.txt", "bar\n")
        self.repo.commit("commit3")
        self.repo.update(self.commit1)
        self.repo.write_file("hello.txt", "hola\n")
        self.repo.write_file("a.cpp", "//comment \n")

        added = set()
        removed = set()
        modified = set()

        async with self.get_streaming_client() as client:
            sub_params = StreamChangesSinceParams(
                mountPoint=self.mount_path_bytes,
                fromPosition=before,
            )
            globs = ["foo/**/*.txt"]  # only foo/bar.txt should be selected
            params = StreamSelectedChangesSinceParams(
                changesParams=sub_params, globs=globs
            )
            result, changes = await client.streamSelectedChangesSince(params)
            print(changes)
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

        # they changed but we don't subscribe to them
        self.assertNotIn("a.cpp", added)
        self.assertNotIn("b.cpp", added)
        self.assertNotIn("bar.txt", added)
        self.assertNotIn("hello.txt", modified)

        # selected
        self.assertIn("foo/bar.txt", removed)

        self.assertNotEqual(before, result.toPosition)

    async def test_journal_stream_selected_changes_since(self) -> None:
        """
        Verify that the streamSelectedChangesSince API can handle complex globs correctly
        """
        async with self.get_streaming_client() as client:
            before = await client.getCurrentJournalPosition(self.mount_path_bytes)
        self.repo.write_file("b.cpp", "//comment\n")
        self.repo.commit("commit3")
        self.repo.update(self.commit1)
        self.repo.write_file("hello.txt", "hola\n")
        self.repo.write_file("bar.txt", "bar\n")
        self.repo.write_file("a.cpp", "//comment \n")

        added = set()
        removed = set()
        modified = set()

        async with self.get_streaming_client() as client:
            sub_params = StreamChangesSinceParams(
                mountPoint=self.mount_path_bytes,
                fromPosition=before,
            )
            globs = ["*.cpp"]
            params = StreamSelectedChangesSinceParams(
                changesParams=sub_params, globs=globs
            )
            result, changes = await client.streamSelectedChangesSince(params)
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

        # We only care about cpp file
        self.assertIn("a.cpp", added)
        self.assertIn("b.cpp", added)

        # we don't care
        self.assertNotIn("bar.txt", added)
        self.assertNotIn("foo/bar.txt", removed)
        self.assertNotIn("foo", removed)

        self.assertNotEqual(before, result.toPosition)

    async def test_journal_stream_selected_changes_since_empty_glob(self) -> None:
        """
        For streamSelectedChangesSince API, empty glob should report no changes
        """
        async with self.get_streaming_client() as client:
            before = await client.getCurrentJournalPosition(self.mount_path_bytes)

        added = set()
        removed = set()
        modified = set()
        self.repo.write_file("b.cpp", "//comment\n")

        async with self.get_streaming_client() as client:
            sub_params = StreamChangesSinceParams(
                mountPoint=self.mount_path_bytes,
                fromPosition=before,
            )
            globs = []
            params = StreamSelectedChangesSinceParams(
                changesParams=sub_params, globs=globs
            )
            result, changes = await client.streamSelectedChangesSince(params)
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

        # no changes
        self.assertNotIn("bar.txt", added)
        self.assertNotIn("foo/bar.txt", removed)
        self.assertNotIn("foo", removed)
        self.assertNotIn("b.cpp", added)

        # however journal is moved
        self.assertNotEqual(before, result.toPosition)

    async def test_journal_stream_changes_since(self) -> None:
        """
        Verify that the streamChangesSince API reports all the changed
        files/directories across update.
        """

        # This is the only integration test that needs Thrift streaming
        # support, which currently is not supported in the open source build.

        async with self.get_streaming_client() as client:
            before = await client.getCurrentJournalPosition(self.mount_path_bytes)

        self.repo.update(self.commit1)

        self.repo.write_file("hello.txt", "hola\n")
        self.repo.write_file("bar.txt", "bar\n")

        added = set()
        removed = set()
        modified = set()

        async with self.get_streaming_client() as client:
            params = StreamChangesSinceParams(
                mountPoint=self.mount_path_bytes,
                fromPosition=before,
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

        self.assertNotEqual(before, result.toPosition)

        counter_name = (
            "thrift.StreamingEdenService.streamChangesSince.streaming_time_us.avg.60"
        )
        counters = self.get_counters()
        self.assertIn(counter_name, counters)
        self.assertGreater(counters[counter_name], 0)
