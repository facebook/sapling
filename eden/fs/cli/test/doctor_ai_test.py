#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import argparse
import subprocess
import unittest
from typing import Any, Optional
from unittest.mock import MagicMock, patch

from eden.fs.cli import telemetry

from ..main import CLAUDE_TIMEOUT_SECS, DoctorAICmd


class DoctorAITest(unittest.TestCase):
    def setUp(self) -> None:
        self.logger = telemetry.TestTelemetryLogger()
        # samples is a class attribute shared by every logger instance in the
        # process, so clear it on the class instead of shadowing it per instance.
        type(self.logger).samples.clear()
        self.instance = MagicMock()
        self.instance.get_telemetry_logger.return_value = self.logger

    def run_cmd(
        self,
        doctor_returncode: int,
        doctor_text: str = "",
        claude_run: Optional[Any] = None,
        doctor_raises: Optional[BaseException] = None,
    ) -> int:
        def cure_what_ails_you(*args: Any, out: Any = None, **kwargs: Any) -> int:
            if doctor_raises is not None:
                raise doctor_raises
            if doctor_text:
                out.writeln(doctor_text)
            return doctor_returncode

        args = argparse.Namespace(debug=False, claude_timeout_secs=CLAUDE_TIMEOUT_SECS)
        cmd = DoctorAICmd(MagicMock(spec=argparse.ArgumentParser))
        with patch("eden.fs.cli.main.get_eden_instance", return_value=self.instance):
            with patch(
                "eden.fs.cli.doctor.cure_what_ails_you", side_effect=cure_what_ails_you
            ):
                with patch(
                    "eden.fs.cli.main.subprocess.run",
                    side_effect=claude_run or (lambda *a, **kw: None),
                ) as mock_run:
                    returncode = cmd.run(args)
        self.mock_run = mock_run
        return returncode

    def last_sample(self) -> telemetry.JsonTelemetrySample:
        self.assertEqual(1, len(self.logger.samples))
        return self.logger.samples[0]

    def test_healthy_doctor_does_not_invoke_claude(self) -> None:
        self.assertEqual(0, self.run_cmd(doctor_returncode=0))
        self.mock_run.assert_not_called()

        sample = self.last_sample()
        self.assertEqual("eden_doctor_ai", sample.strings["type"])
        self.assertEqual("doctor_ok", sample.strings["reason"])
        self.assertEqual(0, sample.ints["exit_code"])
        # `duration` and `success` describe the claude step, which never ran.
        self.assertNotIn("duration", sample.doubles)
        self.assertNotIn("success", sample.ints)

    def test_successful_diagnosis(self) -> None:
        claude_result = subprocess.CompletedProcess(
            args=["claude", "--print"], returncode=0, stdout="diagnosis", stderr=""
        )
        self.assertEqual(
            1,
            self.run_cmd(
                doctor_returncode=1,
                doctor_text="problem found",
                claude_run=lambda *a, **kw: claude_result,
            ),
        )

        sample = self.last_sample()
        self.assertEqual("claude_ok", sample.strings["reason"])
        self.assertEqual(1, sample.ints["success"])
        self.assertEqual(1, sample.ints["exit_code"])
        self.assertIn("duration", sample.doubles)

    def test_failed_diagnosis(self) -> None:
        claude_result = subprocess.CompletedProcess(
            args=["claude", "--print"], returncode=2, stdout="", stderr="boom"
        )
        self.run_cmd(doctor_returncode=1, claude_run=lambda *a, **kw: claude_result)

        sample = self.last_sample()
        self.assertEqual("claude_failed", sample.strings["reason"])
        self.assertEqual(0, sample.ints["success"])

    def test_claude_not_on_path(self) -> None:
        self.run_cmd(doctor_returncode=1, claude_run=FileNotFoundError)

        sample = self.last_sample()
        self.assertEqual("claude_not_on_path", sample.strings["reason"])
        self.assertEqual(0, sample.ints["success"])
        self.assertNotIn("duration", sample.doubles)

    def test_claude_not_executable(self) -> None:
        self.run_cmd(
            doctor_returncode=1,
            claude_run=PermissionError(13, "Permission denied"),
        )

        sample = self.last_sample()
        self.assertEqual("claude_failed", sample.strings["reason"])
        self.assertEqual(0, sample.ints["success"])
        self.assertIn("duration", sample.doubles)

    def test_claude_timeout(self) -> None:
        self.run_cmd(
            doctor_returncode=1,
            claude_run=subprocess.TimeoutExpired(
                cmd=["claude", "--print"], timeout=CLAUDE_TIMEOUT_SECS
            ),
        )

        sample = self.last_sample()
        self.assertEqual("claude_timed_out", sample.strings["reason"])
        self.assertEqual(0, sample.ints["success"])
        self.assertIn("duration", sample.doubles)

    def test_doctor_raising_still_logs_a_complete_sample(self) -> None:
        with self.assertRaises(RuntimeError):
            self.run_cmd(
                doctor_returncode=1, doctor_raises=RuntimeError("doctor exploded")
            )

        sample = self.last_sample()
        self.assertEqual("unhandled_exception", sample.strings["reason"])
        self.assertEqual(-1, sample.ints["exit_code"])
        self.assertEqual(0, sample.ints["success"])
        self.assertEqual("doctor exploded", sample.strings["error"])

    def test_interrupt_is_recorded_separately(self) -> None:
        with self.assertRaises(KeyboardInterrupt):
            self.run_cmd(doctor_returncode=1, doctor_raises=KeyboardInterrupt())

        sample = self.last_sample()
        self.assertEqual("interrupted", sample.strings["reason"])
        self.assertEqual(-1, sample.ints["exit_code"])
        self.assertEqual(0, sample.ints["success"])
