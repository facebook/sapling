#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import argparse
import os
import unittest
from unittest.mock import MagicMock

from eden.test_support.temporary_directory import TemporaryDirectoryMixin

from ..main import HealthReportCmd


class HealthReportTest(unittest.TestCase, TemporaryDirectoryMixin):
    def test_calling_into_health_report(self) -> None:
        temp_dir = self.make_temporary_directory()
        eden_path = os.path.join(temp_dir, "mount_dir")

        mock_argument_parser = MagicMock(spec=argparse.ArgumentParser)
        args = argparse.Namespace(mount=eden_path, only_repo_source=True)

        test_health_report_cmd = HealthReportCmd(mock_argument_parser)
        result = test_health_report_cmd.run(args)

        assert result == 0
