#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import io
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import MagicMock, patch

from eden.fs.cli import daemon, daemon_util
from eden.fs.cli.config import EdenInstance


class StartupLogTest(unittest.TestCase):
    def test_new_startup_log_rotates_the_earlier_log(self) -> None:
        instance: MagicMock = MagicMock(spec=EdenInstance)
        with tempfile.TemporaryDirectory() as temp_dir:
            instance.state_dir = Path(temp_dir)
            earlier = instance.state_dir / daemon_util.STARTUP_LOG_FILENAME
            earlier.write_text("earlier start\n")

            startup_log = daemon._new_startup_log(instance)

            self.assertEqual(startup_log, earlier)
            self.assertFalse(startup_log.exists())
            rotated = list(instance.state_dir.glob(f"{earlier.name}.*"))
            self.assertEqual(len(rotated), 1)
            self.assertEqual(rotated[0].read_text(), "earlier start\n")

    def test_read_startup_log_skips_an_older_log(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            startup_log = Path(temp_dir) / daemon_util.STARTUP_LOG_FILENAME
            self.assertIsNone(daemon._read_startup_log(startup_log, 1000.0))

            startup_log.write_text("startup output\n")
            os.utime(startup_log, (999.0, 999.0))
            self.assertIsNone(daemon._read_startup_log(startup_log, 1000.0))

            os.utime(startup_log, (1000.0, 1000.0))
            self.assertEqual(
                daemon._read_startup_log(startup_log, 1000.0), "startup output\n"
            )

    def test_read_startup_log_returns_none_on_a_read_error(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            startup_log = Path(temp_dir) / daemon_util.STARTUP_LOG_FILENAME
            startup_log.write_text("startup output\n")
            with (
                patch.object(Path, "read_text", side_effect=OSError("I/O error")),
                patch.object(daemon, "print_stderr") as print_stderr,
            ):
                self.assertIsNone(daemon._read_startup_log(startup_log, 0.0))

        print_stderr.assert_called_once_with(
            "warning: failed to read startup log: I/O error"
        )

    def test_systemctl_start_echoes_only_a_non_empty_startup_log(self) -> None:
        instance: MagicMock = MagicMock(spec=EdenInstance)
        completed = MagicMock(returncode=0, stderr="")
        for content in [None, "", "startup output\n"]:
            with self.subTest(content=content):
                with (
                    tempfile.TemporaryDirectory() as temp_dir,
                    patch.object(
                        daemon, "_get_systemd_unit", return_value="edenfs@test"
                    ),
                    patch.object(daemon.subprocess, "run", return_value=completed),
                    patch.object(daemon, "_read_startup_log", return_value=content),
                    patch.object(daemon.sys, "stderr", io.StringIO()) as stderr,
                ):
                    instance.state_dir = Path(temp_dir)
                    rc = daemon._systemctl_start_or_reload(instance, {}, False)

                self.assertEqual(rc, 0)
                self.assertEqual(stderr.getvalue(), content or "")
