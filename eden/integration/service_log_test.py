#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import pathlib

from .lib.fake_edenfs import FakeEdenFS
from .lib.service_test_case import (
    ManagedFakeEdenFSMixin,
    ServiceTestCaseBase,
    service_test,
)
from .start_test import run_eden_start_with_real_daemon


# pyre-fixme[38]: `ServiceLogTestBase` does not implement all inherited abstract
#  methods.
class ServiceLogTestBase(ServiceTestCaseBase):
    """Test how the EdenFS service stores its logs.
    """

    def setUp(self) -> None:
        super().setUp()
        self.eden_dir = pathlib.Path(self.make_temporary_directory())

    @property
    def log_file_path(self) -> pathlib.Path:
        return self.eden_dir / "logs" / "edenfs.log"


@service_test
# pyre-fixme[38]: `ServiceLogFakeEdenFSTest` does not implement all inherited
#  abstract methods.
class ServiceLogFakeEdenFSTest(ServiceLogTestBase):
    def test_fake_edenfs_writes_logs_to_file_in_eden_dir(self) -> None:
        self.assertFalse(
            self.log_file_path.exists(),
            f"{self.log_file_path} should not exist before starting fake_edenfs",
        )
        with self.spawn_fake_edenfs(self.eden_dir):
            self.assertTrue(
                self.log_file_path.exists(),
                f"fake_edenfs should create {self.log_file_path}",
            )
            self.assertIn("Starting fake edenfs daemon", self.log_file_path.read_text())

    def test_fake_edenfs_appends_to_existing_log_file(self) -> None:
        self.log_file_path.parent.mkdir(exist_ok=True, parents=True)
        self.log_file_path.write_text("test log messages\n")
        with self.spawn_fake_edenfs(self.eden_dir):
            pass
        self.assertIn("test log messages", self.log_file_path.read_text())


class ServiceLogRealEdenFSTest(ManagedFakeEdenFSMixin, ServiceLogTestBase):
    def test_real_edenfs_writes_logs_to_file_in_eden_dir(self) -> None:
        self.assertFalse(
            self.log_file_path.exists(),
            f"{self.log_file_path} should not exist before starting edenfs",
        )
        run_eden_start_with_real_daemon(
            eden_dir=self.eden_dir,
            etc_eden_dir=self.etc_eden_dir,
            home_dir=self.home_dir,
            systemd=False,
        )
        with FakeEdenFS.from_existing_process(eden_dir=self.eden_dir):
            self.assertTrue(
                self.log_file_path.exists(),
                f"edenfs should create {self.log_file_path}",
            )
            self.assertIn("Starting edenfs", self.log_file_path.read_text())
