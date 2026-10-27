# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Tests for the shared file-result helpers."""

from __future__ import annotations

import os
import tempfile
import unittest
from unittest import mock

from eden.integration.correctness.exploratory.edenfs import file_results
from eden.integration.correctness.exploratory.edenfs.file_results import (
    read_all,
    status_entries,
)


class _Chunk(bytes):
    """Bytes that count how often anything measures them."""

    measured = 0

    def __len__(self) -> int:
        _Chunk.measured += 1
        return super().__len__()


class ReadAllTest(unittest.TestCase):
    def test_reads_every_chunk_at_its_running_offset(self) -> None:
        data = os.urandom(3 * 1024 + 17)
        with tempfile.TemporaryFile() as handle:
            handle.write(data)
            handle.flush()
            before = os.lseek(handle.fileno(), 5, os.SEEK_SET)
            with mock.patch.object(file_results, "READ_CHUNK", 1024):
                self.assertEqual(data, read_all(handle.fileno()))
            self.assertEqual(before, os.lseek(handle.fileno(), 0, os.SEEK_CUR))

    def test_each_chunk_is_measured_a_constant_number_of_times(self) -> None:
        """The offset is a running total: work is linear in the chunk count.

        Summing every earlier chunk's length for each new offset measures
        chunk k about n - k times, n(n+1)/2 in all; a running total
        measures each chunk once.
        """
        chunks = 2000
        offsets: list[int] = []

        def pread(_fd: int, size: int, offset: int) -> bytes:
            offsets.append(offset)
            index = len(offsets) - 1
            return _Chunk(b"x") if index < chunks else b""

        _Chunk.measured = 0
        with mock.patch.object(file_results.os, "pread", pread):
            self.assertEqual(b"x" * chunks, read_all(7))
        self.assertEqual(list(range(chunks + 1)), offsets)
        # One measurement for the offset and at most a few for the join.
        self.assertLessEqual(_Chunk.measured, 3 * chunks)


class StatusEntriesTest(unittest.TestCase):
    def test_entries_are_sorted_path_code_objects(self) -> None:
        self.assertEqual(
            [{"path": "a", "code": "M"}, {"path": "b/c", "code": "?"}],
            status_entries({"b/c": "?", "a": "M"}),
        )
        self.assertEqual([], status_entries({}))
