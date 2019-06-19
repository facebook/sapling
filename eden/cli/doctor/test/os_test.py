#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import typing

from eden.cli.config import EdenInstance
from eden.cli.doctor import check_os
from eden.cli.doctor.test.lib.fake_eden_instance import FakeEdenInstance
from eden.cli.doctor.test.lib.testcase import DoctorTestBase


class OperatingSystemsCheckTest(DoctorTestBase):
    def setUp(self) -> None:
        test_config = {
            "doctor.minimum-kernel-version": "4.11.3-67",
            "doctor.known-bad-kernel-versions": "TODO,TEST",
        }
        tmp_dir = self.make_temporary_directory()
        self.instance = FakeEdenInstance(tmp_dir, config=test_config)

    def test_kernel_version_split(self) -> None:
        test_versions = (
            ("1", (1, 0, 0, 0)),
            ("1.2", (1, 2, 0, 0)),
            ("1.2.3", (1, 2, 3, 0)),
            ("1.2.3.4", (1, 2, 3, 4)),
            ("1.2.3-4", (1, 2, 3, 4)),
            ("1.2.3.4-abc", (1, 2, 3, 4)),
            ("1.2.3-4.abc", (1, 2, 3, 4)),
            ("1.2.3.4-abc.def", (1, 2, 3, 4)),
            ("1.2.3-4.abc-def", (1, 2, 3, 4)),
        )
        for test_version, expected in test_versions:
            with self.subTest(test_version=test_version):
                result = check_os._parse_os_kernel_version(test_version)
                self.assertEquals(result, expected)

    def test_kernel_version_min(self) -> None:
        # Each of these are ((test_value, expected_result), ...)
        min_kernel_versions_tests = (
            ("4.6.7-73_fbk21_3608_gb5941a6", True),
            ("4.6", True),
            ("4.11", True),
            ("4.11.3", True),
            ("4.11.3.66", True),
            ("4.11.3-52_fbk13", True),
            ("4.11.3-77_fbk20_4162_g6e876878d18e", False),
            ("4.11.3-77", False),
        )
        for fake_release, expected in min_kernel_versions_tests:
            with self.subTest(fake_release=fake_release):
                result = check_os._os_is_kernel_version_too_old(
                    typing.cast(EdenInstance, self.instance), fake_release
                )
                self.assertIs(result, expected)

    def test_bad_kernel_versions(self) -> None:
        kernel_versions_tests = {
            "999.2.3-4_TEST": True,
            "777.1_TODO": True,
            "4.16.18-151_fbk13": False,
        }
        for release, is_bad in kernel_versions_tests.items():
            with self.subTest(release=release):
                result = check_os._os_is_bad_release(
                    typing.cast(EdenInstance, self.instance), release
                )
                self.assertEqual(result, is_bad)

    def test_custom_kernel_names(self) -> None:
        custom_name = "4.16.18-custom_byme_3744_g7833bc918498"
        instance = typing.cast(EdenInstance, self.instance)
        self.assertFalse(check_os._os_is_kernel_version_too_old(instance, custom_name))
        self.assertFalse(check_os._os_is_bad_release(instance, custom_name))
