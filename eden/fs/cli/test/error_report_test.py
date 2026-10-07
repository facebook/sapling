#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import tempfile
import unittest
from pathlib import Path
from typing import Optional
from unittest.mock import patch

from eden.fs.cli import error_report
from eden.fs.cli.error_report import ERROR_REPORT_ENV, ErrorReport


class ErrorReportTest(unittest.TestCase):
    def setUp(self) -> None:
        temp_dir = tempfile.TemporaryDirectory()
        self.addCleanup(temp_dir.cleanup)
        self.report_path: Path = Path(temp_dir.name) / "report"
        self.report_path.write_text("")

    def write_report(
        self, exit_code: int, message: Optional[str], exc: Optional[Exception] = None
    ) -> str:
        report = ErrorReport(str(self.report_path))
        if message is not None:
            report.record(message, exc)
        report.write(exit_code)
        return self.report_path.read_text()

    def test_writes_exception_type_and_message(self) -> None:
        self.assertEqual(
            self.write_report(70, "daemon failed", ValueError("daemon failed")),
            "ValueError: daemon failed",
        )

    def test_writes_exception_type_without_message(self) -> None:
        self.assertEqual(self.write_report(1, "", ValueError()), "ValueError")

    def test_writes_message_without_exception(self) -> None:
        self.assertEqual(self.write_report(1, "restart failed"), "restart failed")

    def test_writes_last_recorded_error_first(self) -> None:
        report = ErrorReport(str(self.report_path))
        report.record("startup failed")
        report.record("restart failed", ValueError("restart failed"))
        report.write(1)

        self.assertEqual(
            self.report_path.read_text(), "ValueError: restart failed: startup failed"
        )

    def test_ignores_repeated_error(self) -> None:
        report = ErrorReport(str(self.report_path))
        report.record("startup failed")
        report.record("restart failed")
        report.record("startup failed")
        report.write(1)

        self.assertEqual(self.report_path.read_text(), "restart failed: startup failed")

    def test_writes_nothing_on_success(self) -> None:
        self.assertEqual(self.write_report(0, "ignored"), "")

    def test_writes_nothing_without_recorded_error(self) -> None:
        self.assertEqual(self.write_report(1, None), "")

    def test_does_not_create_missing_file(self) -> None:
        self.report_path.unlink()

        report = ErrorReport(str(self.report_path))
        report.record("lost")
        report.write(1)

        self.assertFalse(self.report_path.exists())

    def test_init_removes_env_var(self) -> None:
        self.addCleanup(error_report.init)
        with patch.dict(os.environ, {ERROR_REPORT_ENV: str(self.report_path)}):
            error_report.init()
            self.assertNotIn(ERROR_REPORT_ENV, os.environ)

        error_report.record("from module")
        error_report.write(1)

        self.assertEqual(self.report_path.read_text(), "from module")
