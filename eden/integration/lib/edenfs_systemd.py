#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import abc
import contextlib
import pathlib
import typing
import unittest
from typing import Optional

from eden.fs.cli.systemd import edenfs_systemd_service_name
from eden.test_support.temporary_directory import TempFileManager

from .find_executables import FindExe
from .systemd import SystemdService, SystemdUserServiceManager, temp_systemd


class EdenFSSystemdMixin(metaclass=abc.ABCMeta):
    systemd: Optional[SystemdUserServiceManager] = None

    if typing.TYPE_CHECKING:
        # Subclasses must provide the following member variables
        exit_stack: contextlib.ExitStack
        temp_mgr: TempFileManager

    def set_up_edenfs_systemd_service(self) -> None:
        systemd = self.systemd
        assert self.systemd is None
        systemd = self.make_temporary_systemd_user_service_manager()
        self.systemd = systemd
        systemd.enable_runtime_unit_from_file(
            # pyre-ignore[6]: T38947910
            unit_file=pathlib.Path(FindExe.SYSTEMD_FB_EDENFS_SERVICE)
        )
        for name, value in systemd.extra_env.items():
            self.setenv(name, value)

    def make_temporary_systemd_user_service_manager(self) -> SystemdUserServiceManager:
        return self.exit_stack.enter_context(temp_systemd(self.temp_mgr))

    def get_edenfs_systemd_service(self, eden_dir: pathlib.Path) -> SystemdService:
        systemd = self.systemd
        assert systemd is not None
        return systemd.get_service(edenfs_systemd_service_name(eden_dir))

    @abc.abstractmethod
    def setenv(self, name: str, value: Optional[str]) -> None:
        raise NotImplementedError()

    def assert_systemd_service_is_active(self, eden_dir: pathlib.Path) -> None:
        service = self.get_edenfs_systemd_service(eden_dir=eden_dir)
        assert isinstance(self, unittest.TestCase)
        self.assertEqual(
            (service.query_active_state(), service.query_sub_state()),
            ("active", "running"),
            f"EdenFS systemd service ({service}) should be running",
        )

    def assert_systemd_service_is_failed(self, eden_dir: pathlib.Path) -> None:
        service = self.get_edenfs_systemd_service(eden_dir=eden_dir)
        assert isinstance(self, unittest.TestCase)
        self.assertEqual(
            (service.query_active_state(), service.query_sub_state()),
            ("failed", "failed"),
            f"EdenFS systemd service ({service}) should have failed",
        )

    def assert_systemd_service_is_stopped(self, eden_dir: pathlib.Path) -> None:
        service = self.get_edenfs_systemd_service(eden_dir=eden_dir)
        assert isinstance(self, unittest.TestCase)
        self.assertEqual(
            (service.query_active_state(), service.query_sub_state()),
            ("inactive", "dead"),
            f"EdenFS systemd service ({service}) should be stopped",
        )
