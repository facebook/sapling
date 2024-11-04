#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import argparse
import os
import unittest
from unittest.mock import MagicMock, patch

from eden.test_support.temporary_directory import TemporaryDirectoryMixin

from ..main import HealthReportCmd


class HealthReportTest(unittest.TestCase, TemporaryDirectoryMixin):
    @patch("eden.fs.cli.util.HealthStatus.is_healthy")
    def test_calling_into_health_report(self, mock_is_healthy: MagicMock) -> None:
        temp_dir = self.make_temporary_directory()
        eden_path = os.path.join(temp_dir, "mount_dir")

        mock_argument_parser = MagicMock(spec=argparse.ArgumentParser)
        args = argparse.Namespace(
            config_dir="/home/johndoe/.eden",
            etc_eden_dir="/etc/eden",
            home_dir="/home/johndoe",
            mount=eden_path,
            only_repo_source=True,
        )
        mock_is_healthy.return_value = True

        test_health_report_cmd = HealthReportCmd(mock_argument_parser)
        result = test_health_report_cmd.run(args)

        assert result == 0

    @patch("eden.fs.cli.util.HealthStatus.is_healthy")
    def test_health_report_notify_eden_not_running(
        self, mock_is_healthy: MagicMock
    ) -> None:
        temp_dir = self.make_temporary_directory()
        eden_path = os.path.join(temp_dir, "mount_dir")

        args = argparse.Namespace(
            config_dir="/home/johndoe/.eden",
            etc_eden_dir="/etc/eden",
            home_dir="/home/johndoe",
            mount=eden_path,
            only_repo_source=True,
        )
        mock_argument_parser = MagicMock(spec=argparse.ArgumentParser)

        mock_is_healthy.return_value = False

        test_health_report_cmd = HealthReportCmd(mock_argument_parser)
        result = test_health_report_cmd.run(args)

        assert result == 1
