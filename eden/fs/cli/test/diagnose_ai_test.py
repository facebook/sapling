#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import argparse
import subprocess
import unittest
import uuid
from typing import Any, Optional
from unittest.mock import MagicMock, patch

from eden.fs.cli import telemetry

from ..ai_diagnosis import CLAUDE_INVESTIGATION_TIMEOUT_SECS
from ..main import DiagnoseAICmd


class DiagnoseAITest(unittest.TestCase):
    def setUp(self) -> None:
        self.logger = telemetry.TestTelemetryLogger()
        # samples is a class attribute shared by every logger instance in the
        # process, so clear it on the class instead of shadowing it per instance.
        type(self.logger).samples.clear()
        self.instance = MagicMock()
        self.instance.get_telemetry_logger.return_value = self.logger

    def run_cmd(
        self,
        symptom: list[str],
        claude_run: Optional[Any] = None,
    ) -> int:
        args = argparse.Namespace(
            debug=False,
            symptom=symptom,
            claude_timeout_secs=CLAUDE_INVESTIGATION_TIMEOUT_SECS,
        )
        cmd = DiagnoseAICmd(MagicMock(spec=argparse.ArgumentParser))
        with patch("eden.fs.cli.main.get_eden_instance", return_value=self.instance):
            with patch(
                "eden.fs.cli.ai_diagnosis.subprocess.run",
                side_effect=claude_run or (lambda *a, **kw: None),
            ) as mock_run:
                returncode = cmd.run(args)
        self.mock_run = mock_run
        return returncode

    def last_sample(self) -> telemetry.JsonTelemetrySample:
        self.assertEqual(1, len(self.logger.samples))
        return self.logger.samples[0]

    def test_successful_diagnosis(self) -> None:
        claude_result = subprocess.CompletedProcess(
            args=["claude", "--print"], returncode=0, stdout="diagnosis", stderr=""
        )
        self.assertEqual(
            0,
            self.run_cmd(
                ["sl", "status", "is", "slow"],
                claude_run=lambda *a, **kw: claude_result,
            ),
        )

        # The symptom reaches claude verbatim, and names the skill that routes
        # on it -- an unrouted prompt would still exit 0 and look healthy here.
        prompt = self.mock_run.call_args.kwargs["input"]
        self.assertIn("diagnose-sapling", prompt)
        self.assertIn("sl status is slow", prompt)

        sample = self.last_sample()
        self.assertEqual("eden_diagnose_ai", sample.strings["type"])
        self.assertEqual("claude_ok", sample.strings["reason"])
        self.assertEqual(1, sample.ints["success"])
        self.assertIn("duration", sample.doubles)

        session_id = sample.strings["claude_session_id"]
        uuid.UUID(session_id)
        self.assertEqual(
            ["claude", "--print", "--session-id", session_id],
            self.mock_run.call_args.args[0],
        )

    def test_no_diagnosis_exits_nonzero(self) -> None:
        # Unlike doctor-ai, there is no doctor exit code to pass through, so a
        # claude that never answers is the command's own failure.
        self.assertEqual(1, self.run_cmd(["sl", "pull", "hangs"], FileNotFoundError))

        sample = self.last_sample()
        self.assertEqual("claude_not_on_path", sample.strings["reason"])
        self.assertEqual(0, sample.ints["success"])

    def test_interrupt_still_logs_a_sample(self) -> None:
        with self.assertRaises(KeyboardInterrupt):
            self.run_cmd(["sl", "status", "is", "slow"], KeyboardInterrupt)

        sample = self.last_sample()
        self.assertEqual("interrupted", sample.strings["reason"])
