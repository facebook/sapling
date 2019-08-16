#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import abc
import pathlib
import typing
import unittest

from eden.cli.systemd import edenfs_systemd_service_name

from .find_executables import FindExe
from .systemd import SystemdService, SystemdUserServiceManager


class EdenFSSystemdMixin(metaclass=abc.ABCMeta):
    systemd: typing.Optional[SystemdUserServiceManager] = None

    def set_up_edenfs_systemd_service(self) -> None:
        assert self.systemd is None
        self.systemd = self.make_temporary_systemd_user_service_manager()
        self.systemd.enable_runtime_unit_from_file(
            unit_file=pathlib.Path(
                typing.cast(str, FindExe.SYSTEMD_FB_EDENFS_SERVICE)  # T38947910
            )
        )
        self.set_environment_variables(self.systemd.extra_env)

    def get_edenfs_systemd_service(self, eden_dir: pathlib.Path) -> SystemdService:
        assert self.systemd is not None
        # pyre-fixme[16]: Optional type has no attribute `get_service`.
        return self.systemd.get_service(edenfs_systemd_service_name(eden_dir))

    @abc.abstractmethod
    def make_temporary_systemd_user_service_manager(self) -> SystemdUserServiceManager:
        raise NotImplementedError()

    @abc.abstractmethod
    def set_environment_variables(self, variables: typing.Mapping[str, str]) -> None:
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
