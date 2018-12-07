#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

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
            unit_file=pathlib.Path(FindExe.SYSTEMD_FB_EDENFS_SERVICE)
        )
        self.set_environment_variables(self.systemd.extra_env)

    def get_edenfs_systemd_service(self, eden_dir: pathlib.Path) -> SystemdService:
        assert self.systemd is not None
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

    def assert_systemd_service_is_stopped(self, eden_dir: pathlib.Path) -> None:
        service = self.get_edenfs_systemd_service(eden_dir=eden_dir)
        assert isinstance(self, unittest.TestCase)
        self.assertEqual(
            (service.query_active_state(), service.query_sub_state()),
            ("inactive", "dead"),
            f"EdenFS systemd service ({service}) should be stopped",
        )
