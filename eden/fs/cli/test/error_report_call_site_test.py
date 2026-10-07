#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import argparse
import os
import tempfile
import unittest
from pathlib import Path
from typing import Optional
from unittest.mock import MagicMock, patch

from eden.fs.cli import config as config_mod, error_report, main as main_mod, telemetry
from eden.fs.cli.error_report import ERROR_REPORT_ENV


class ErrorReportCallSiteTest(unittest.TestCase):
    def setUp(self) -> None:
        temp_dir = tempfile.TemporaryDirectory()
        self.addCleanup(temp_dir.cleanup)
        self.report_path: Path = Path(temp_dir.name) / "report"
        self.report_path.write_text("")
        self.addCleanup(error_report.init)

    def start_report(self) -> None:
        with patch.dict(os.environ, {ERROR_REPORT_ENV: str(self.report_path)}):
            error_report.init()

    def finish_report(self, exit_code: int) -> str:
        error_report.write(exit_code)
        return self.report_path.read_text()


class RestartErrorReportTest(ErrorReportCallSiteTest):
    def make_restart_cmd(self) -> main_mod.RestartCmd:
        restart_cmd = main_mod.RestartCmd(argparse.ArgumentParser())
        restart_cmd.args = argparse.Namespace(
            allow_root=False,
            daemon_binary=None,
            force_restart=False,
            migrate_to=None,
            only_if_running=False,
            preserved_vars=None,
            prompt=False,
            restart_type=None,
        )
        return restart_cmd

    def make_telemetry_logger(self) -> telemetry.TestTelemetryLogger:
        telemetry_logger = telemetry.TestTelemetryLogger()
        telemetry_logger.samples = []
        return telemetry_logger

    def run_recovery(
        self,
        pid: Optional[int],
        old_daemon_resumes: bool,
        start_code: int,
        finish_restart_code: int,
    ) -> tuple[int, str]:
        """Run the recovery after a failed graceful restart.

        Return the exit code and the `error` string of the telemetry sample.
        """
        restart_cmd = self.make_restart_cmd()
        telemetry_logger = self.make_telemetry_logger()
        sample = telemetry_logger.new_sample("graceful_restart")
        instance = MagicMock()
        instance.check_health.return_value = MagicMock(pid=pid)

        with (
            patch.object(
                main_mod,
                "wait_for_instance_healthy",
                side_effect=None if old_daemon_resumes else TimeoutError(),
            ),
            patch.object(main_mod.os, "kill"),
            patch.object(restart_cmd, "_wait_for_stop"),
            patch.object(restart_cmd, "_start", return_value=start_code),
            patch.object(
                restart_cmd, "_finish_restart", return_value=finish_restart_code
            ),
        ):
            exit_code = restart_cmd._recover_after_failed_graceful_restart(
                instance, sample
            )

        sample.log()
        return exit_code, telemetry_logger.samples[0].strings["error"]

    def test_recovery_records_error_in_sample_and_report(self) -> None:
        cases = [
            (
                1234,
                True,
                0,
                0,
                1,
                "Graceful restart failed, and old EdenFS process resumed",
            ),
            (
                None,
                False,
                1,
                0,
                1,
                "EdenFS was not running after graceful restart",
            ),
            (
                1234,
                False,
                0,
                0,
                2,
                "EdenFS was not healthy after graceful restart; performed a "
                "hard restart",
            ),
            (
                1234,
                False,
                0,
                1,
                3,
                "EdenFS was not healthy after graceful restart, and we failed "
                "to restart it",
            ),
        ]
        for pid, resumes, start_code, finish_code, want_code, want_error in cases:
            with self.subTest(want_error=want_error):
                self.report_path.write_text("")
                self.start_report()

                exit_code, sample_error = self.run_recovery(
                    pid, resumes, start_code, finish_code
                )

                self.assertEqual(exit_code, want_code)
                self.assertEqual(sample_error, want_error)
                self.assertEqual(self.finish_report(exit_code), want_error)

    def test_failed_takeover_records_outcome_and_cause(self) -> None:
        restart_cmd = self.make_restart_cmd()
        telemetry_logger = self.make_telemetry_logger()
        instance = MagicMock()
        instance.state_dir = Path("/home/test/.eden")
        instance.get_telemetry_logger.return_value = telemetry_logger
        instance.check_health.return_value = MagicMock(pid=1234)
        self.start_report()

        def fail_takeover(*args: object, **kwargs: object) -> int:
            error_report.record("edenfs daemon startup exited with status 1")
            return 1

        with (
            patch.object(config_mod, "get_transport_mismatches", return_value=[]),
            patch.object(main_mod, "remove_legacyephemeral_checkouts"),
            patch.object(
                main_mod.daemon,
                "gracefully_restart_edenfs_service",
                side_effect=fail_takeover,
            ),
            patch.object(main_mod, "wait_for_instance_healthy"),
        ):
            exit_code = restart_cmd._graceful_restart(instance)

        self.assertEqual(exit_code, 1)
        self.assertEqual(
            self.finish_report(exit_code),
            "Graceful restart failed, and old EdenFS process resumed: "
            "edenfs daemon startup exited with status 1",
        )

    def test_transport_mismatch_restart_failure_is_recorded(self) -> None:
        restart_cmd = self.make_restart_cmd()
        telemetry_logger = self.make_telemetry_logger()
        instance = MagicMock()
        instance.state_dir = Path("/home/test/.eden")
        instance.get_telemetry_logger.return_value = telemetry_logger
        instance.check_health.return_value = MagicMock(pid=1234)
        mismatch = config_mod.TransportMismatch(
            mount=Path("/mnt/eden"),
            active_transport="devfuse",
            desired_transport="io_uring",
        )
        self.start_report()

        with (
            patch.object(
                config_mod, "get_transport_mismatches", return_value=[mismatch]
            ),
            patch.object(restart_cmd, "_full_restart", return_value=1),
        ):
            exit_code = restart_cmd._graceful_restart(instance)

        want_error = "Transport mismatch fallback full restart failed"
        self.assertEqual(exit_code, 1)
        self.assertEqual(telemetry_logger.samples[0].strings["error"], want_error)
        self.assertEqual(self.finish_report(exit_code), want_error)

    def test_unhealthy_daemon_without_force_is_recorded(self) -> None:
        restart_cmd = self.make_restart_cmd()
        instance = MagicMock()
        instance.check_health.return_value = MagicMock(
            pid=1234,
            status=main_mod.fb303_status.STOPPED,
            is_healthy=MagicMock(return_value=False),
        )
        self.start_report()

        with patch.object(main_mod, "get_eden_instance", return_value=instance):
            exit_code = restart_cmd.run(restart_cmd.args)

        self.assertEqual(exit_code, 4)
        self.assertEqual(
            self.finish_report(exit_code),
            "edenfs daemon is not healthy (status STOPPED) and --force was not given",
        )


class MainErrorReportTest(ErrorReportCallSiteTest):
    def test_main_records_unhandled_exception_and_reraises(self) -> None:
        with (
            patch.dict(os.environ, {ERROR_REPORT_ENV: str(self.report_path)}),
            patch.object(main_mod, "_main", side_effect=ValueError("boom")),
        ):
            with self.assertRaises(ValueError):
                main_mod.main()

        self.assertEqual(self.report_path.read_text(), "ValueError: boom")

    def test_main_writes_report_for_non_zero_exit_code(self) -> None:
        def fail() -> int:
            error_report.record("restart failed")
            return 1

        with (
            patch.dict(os.environ, {ERROR_REPORT_ENV: str(self.report_path)}),
            patch.object(main_mod, "_main", side_effect=fail),
        ):
            self.assertEqual(main_mod.main(), 1)

        self.assertEqual(self.report_path.read_text(), "restart failed")
