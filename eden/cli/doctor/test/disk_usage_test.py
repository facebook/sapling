#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import collections
import typing
from typing import List, Optional
from unittest.mock import patch

import eden.cli.doctor as doctor
from eden.cli.config import EdenInstance
from eden.cli.doctor.problem import ProblemBase, ProblemTracker
from eden.cli.doctor.test.lib.fake_eden_instance import FakeEdenInstance
from eden.cli.doctor.test.lib.testcase import DoctorTestBase


class DiskUsageTest(DoctorTestBase):
    def _mock_disk_usage(self, blocks, avail, frsize=1024) -> None:
        """Mock test for disk usage."""
        mock_statvfs_patcher = patch("eden.cli.doctor.os.statvfs")
        mock_statvfs = mock_statvfs_patcher.start()
        self.addCleanup(lambda: mock_statvfs.stop())
        statvfs_tuple = collections.namedtuple("statvfs", "f_blocks f_bavail f_frsize")
        mock_statvfs.return_value = statvfs_tuple(blocks, avail, frsize)

        mock_getmountpt_and_deviceid_patcher = patch(
            "eden.cli.doctor.check_filesystems.get_mountpt"
        )
        mock_getmountpt_and_deviceid = mock_getmountpt_and_deviceid_patcher.start()
        self.addCleanup(lambda: mock_getmountpt_and_deviceid.stop())
        mock_getmountpt_and_deviceid.return_value = "/"

    def _check_disk_usage(
        self, instance: Optional[FakeEdenInstance] = None
    ) -> List[ProblemBase]:
        problem_collector = ProblemCollector()
        if instance is None:
            instance = FakeEdenInstance(self.make_temporary_directory())

        doctor.check_filesystems.check_disk_usage(
            tracker=problem_collector,
            mount_paths=["/"],
            instance=typing.cast(EdenInstance, instance),
        )
        return problem_collector.problems

    def test_low_free_absolute_disk_is_major(self):
        self._mock_disk_usage(blocks=100000000, avail=500000)
        problems = self._check_disk_usage()

        self.assertEqual(
            problems[0].description(),
            "/ has only 512000000 bytes available. "
            "Eden lazily loads your files and needs enough disk "
            "space to store these files when loaded.",
        )
        self.assertEqual(problems[0].severity(), doctor.ProblemSeverity.ERROR)

    def test_low_percentage_free_but_high_absolute_free_disk_is_minor(self):
        self._mock_disk_usage(blocks=100000000, avail=2000000)
        problems = self._check_disk_usage()

        self.assertEqual(
            problems[0].description(),
            "/ is 98.00% full. "
            "Eden lazily loads your files and needs enough disk "
            "space to store these files when loaded.",
        )
        self.assertEqual(problems[0].severity(), doctor.ProblemSeverity.ADVICE)

    def test_high_percentage_free_but_small_disk_is_major(self):
        self._mock_disk_usage(blocks=800000, avail=500000)
        problems = self._check_disk_usage()

        self.assertEqual(
            problems[0].description(),
            "/ has only 512000000 bytes available. "
            "Eden lazily loads your files and needs enough disk "
            "space to store these files when loaded.",
        )
        self.assertEqual(problems[0].severity(), doctor.ProblemSeverity.ERROR)

    def test_disk_usage_normal(self):
        self._mock_disk_usage(blocks=100000000, avail=50000000)
        problems = self._check_disk_usage()
        self.assertEqual(len(problems), 0)

    def test_issue_includes_custom_message_from_config(self) -> None:
        self._mock_disk_usage(blocks=100000000, avail=500000)
        instance = FakeEdenInstance(
            self.make_temporary_directory(),
            config={
                "doctor.low-disk-space-message": "Ask your administrator for help."
            },
        )
        problems = self._check_disk_usage(instance=instance)
        self.assertEqual(
            problems[0].description(),
            "/ has only 512000000 bytes available. "
            "Eden lazily loads your files and needs enough disk "
            "space to store these files when loaded. Ask your administrator "
            "for help.",
        )

        self._mock_disk_usage(blocks=100000000, avail=2000000)
        instance = FakeEdenInstance(
            self.make_temporary_directory(),
            config={
                "doctor.low-disk-space-message": "Ask your administrator for help."
            },
        )
        problems = self._check_disk_usage(instance=instance)
        self.assertEqual(
            problems[0].description(),
            "/ is 98.00% full. "
            "Eden lazily loads your files and needs enough disk "
            "space to store these files when loaded. Ask your administrator "
            "for help.",
        )


class ProblemCollector(ProblemTracker):
    problems: List[ProblemBase]

    def __init__(self) -> None:
        super().__init__()
        self.problems = []

    def add_problem(self, problem: ProblemBase) -> None:
        self.problems.append(problem)
