#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import errno
import os
import tempfile
import unittest
from unittest.mock import MagicMock, patch

from .. import mp, mtab


class MultiprocessingTest(unittest.TestCase):
    def test_context_uses_spawn(self) -> None:
        self.assertEqual("spawn", mp.get_context().get_start_method())

    def test_lstat_process_join_includes_process_startup_grace(self) -> None:
        table = mtab.new()
        proc = MagicMock()
        proc.is_alive.return_value = False
        proc.exitcode = 0
        result_reader = MagicMock()
        result_reader.recv.return_value = errno.ENOENT
        lstat_process = mtab.LstatProcess(
            process=proc,
            result_reader=result_reader,
            result_writer=MagicMock(),
        )

        with patch.object(table, "create_lstat_process", return_value=lstat_process):
            result = table.check_path_access(b"/tmp", b"fuse")

        self.assertIsNone(result)
        proc.join.assert_called_once_with(
            timeout=(
                mtab.kMountStaleSecondsTimeout + mtab.kMountProcessStartupGraceSeconds
            )
        )

    def test_spawned_lstat_process_reports_healthy_mount(self) -> None:
        table = mtab.new()
        with tempfile.TemporaryDirectory() as temp_dir:
            result = table.check_path_access(os.fsencode(temp_dir), b"fuse")

        self.assertIsNone(result)

    def test_child_failure_is_not_interpreted_as_lstat_errno(self) -> None:
        table = mtab.new()
        proc = MagicMock()
        proc.is_alive.return_value = False
        proc.exitcode = 1
        result_reader = MagicMock()
        lstat_process = mtab.LstatProcess(
            process=proc,
            result_reader=result_reader,
            result_writer=MagicMock(),
        )

        with patch.object(table, "create_lstat_process", return_value=lstat_process):
            with self.assertRaises(OSError) as context:
                table.check_path_access(b"/tmp", b"fuse")

        self.assertEqual(errno.EIO, context.exception.errno)
        result_reader.recv.assert_not_called()

    def test_process_start_failure_is_not_treated_as_healthy_mount(self) -> None:
        table = mtab.new()
        proc = MagicMock()
        proc.start.side_effect = FileNotFoundError(
            errno.ENOENT, "could not launch child interpreter"
        )
        lstat_process = mtab.LstatProcess(
            process=proc,
            result_reader=MagicMock(),
            result_writer=MagicMock(),
        )

        with patch.object(table, "create_lstat_process", return_value=lstat_process):
            with self.assertRaises(FileNotFoundError) as context:
                table.check_path_access(b"/tmp", b"fuse")

        self.assertEqual(errno.ENOENT, context.exception.errno)
