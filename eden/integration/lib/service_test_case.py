#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import abc
import pathlib
import sys
import typing
import unittest
from typing import Optional, Type

from eden.test_support.temporary_directory import TemporaryDirectoryMixin

from . import edenclient, testcase
from .fake_edenfs import FakeEdenFS
from .find_executables import FindExe
from .testcase import test_replicator


if sys.platform.startswith("linux"):
    from eden.fs.cli.systemd import edenfs_systemd_service_name

    from .systemd import SystemdService, SystemdUserServiceManager, temp_systemd

    try:
        import pystemd  # noqa: F401 # @manual

        _systemd_supported = True
    except ModuleNotFoundError:
        # The edenfsctl CLI only supports starting with systemd when the pystemd
        # module is available
        _systemd_supported = False
else:
    _systemd_supported = False

    class SystemdUserServiceManager:
        pass

    class SystemdService:
        pass


@unittest.skipIf(not edenclient.can_run_fake_edenfs(), "unable to run fake_edenfs")
class ServiceTestCaseBase(
    testcase.IntegrationTestCase, TemporaryDirectoryMixin, metaclass=abc.ABCMeta
):
    """Abstract base class for tests covering 'eden start', 'eden stop', etc.

    Use the @service_test decorator to make a concrete subclass.
    """

    __etc_eden_dir: typing.Optional[pathlib.Path] = None
    __home_dir: typing.Optional[pathlib.Path] = None
    __tmp_dir: typing.Optional[pathlib.Path] = None

    @abc.abstractmethod
    def spawn_fake_edenfs(
        self,
        eden_dir: pathlib.Path,
        extra_arguments: typing.Optional[typing.Sequence[str]] = None,
    ) -> FakeEdenFS:
        raise NotImplementedError()

    def get_required_eden_cli_args(self) -> typing.List[str]:
        return [
            "--etc-eden-dir",
            str(self.etc_eden_dir),
            "--home-dir",
            str(self.home_dir),
        ]

    @property
    def etc_eden_dir(self) -> pathlib.Path:
        etc_eden_dir = self.__etc_eden_dir
        if etc_eden_dir is None:
            etc_eden_dir = self.make_test_dir("etc_eden")
            self.__etc_eden_dir = etc_eden_dir
        return etc_eden_dir

    @property
    def home_dir(self) -> pathlib.Path:
        home_dir = self.__home_dir
        if home_dir is None:
            home_dir = self.make_test_dir("home")
            self.__home_dir = home_dir
        return home_dir


class AdHocFakeEdenFSMixin(ServiceTestCaseBase):
    """Test by spawning fake_edenfs directly.

    Use the @service_test decorator to use this mixin automatically.
    """

    def spawn_fake_edenfs(
        self,
        eden_dir: pathlib.Path,
        extra_arguments: typing.Optional[typing.Sequence[str]] = None,
    ) -> FakeEdenFS:
        return FakeEdenFS.spawn(
            eden_dir=eden_dir,
            etc_eden_dir=self.etc_eden_dir,
            home_dir=self.home_dir,
            extra_arguments=extra_arguments,
        )


class ManagedFakeEdenFSMixin(ServiceTestCaseBase):
    """Test by using 'eden start' to spawn fake_edenfs.

    Use the @service_test decorator to use this mixin automatically.
    """

    def spawn_fake_edenfs(
        self,
        eden_dir: pathlib.Path,
        extra_arguments: typing.Optional[typing.Sequence[str]] = None,
    ) -> FakeEdenFS:
        # TODO(T33122320): Opt out of using systemd when using systemd is the
        # default option.
        return FakeEdenFS.spawn_via_cli(
            eden_dir=eden_dir,
            etc_eden_dir=self.etc_eden_dir,
            home_dir=self.home_dir,
            extra_arguments=extra_arguments,
        )


class SystemdServiceTest(ServiceTestCaseBase):
    """Test by using 'eden start' with systemd enabled to spawn fake_edenfs.

    Use the @service_test decorator to use this mixin automatically.
    """

    systemd: Optional[SystemdUserServiceManager] = None

    def setUp(self) -> None:
        super().setUp()
        # TODO(T33122320): Don't set EDEN_EXPERIMENTAL_SYSTEMD when using
        # systemd is the default option.
        self.setenv("EDEN_EXPERIMENTAL_SYSTEMD", "1")
        self.set_up_edenfs_systemd_service()

    def spawn_fake_edenfs(
        self,
        eden_dir: pathlib.Path,
        extra_arguments: typing.Optional[typing.Sequence[str]] = None,
    ) -> FakeEdenFS:
        return FakeEdenFS.spawn_via_cli(
            eden_dir=eden_dir,
            etc_eden_dir=self.etc_eden_dir,
            home_dir=self.home_dir,
            extra_arguments=extra_arguments,
        )

    def set_up_edenfs_systemd_service(self) -> None:
        if sys.platform.startswith("linux"):
            systemd = self.systemd
            assert self.systemd is None
            systemd = self.make_temporary_systemd_user_service_manager()
            self.systemd = systemd
            systemd.enable_runtime_unit_from_file(
                unit_file=pathlib.Path(FindExe.SYSTEMD_FB_EDENFS_SERVICE)
            )
            for name, value in systemd.extra_env.items():
                self.setenv(name, value)
        else:
            raise NotImplementedError("systemd not supported on this platform")

    def make_temporary_systemd_user_service_manager(self) -> SystemdUserServiceManager:
        if sys.platform.startswith("linux"):
            return self.exit_stack.enter_context(temp_systemd(self.temp_mgr))
        else:
            raise NotImplementedError("systemd not supported on this platform")

    def get_edenfs_systemd_service(self, eden_dir: pathlib.Path) -> SystemdService:
        if sys.platform.startswith("linux"):
            systemd = self.systemd
            assert systemd is not None
            return systemd.get_service(edenfs_systemd_service_name(eden_dir))
        else:
            raise NotImplementedError("systemd not supported on this platform")

    def assert_systemd_service_is_active(self, eden_dir: pathlib.Path) -> None:
        if sys.platform.startswith("linux"):
            service = self.get_edenfs_systemd_service(eden_dir=eden_dir)
            assert isinstance(self, unittest.TestCase)
            self.assertEqual(
                (service.query_active_state(), service.query_sub_state()),
                ("active", "running"),
                f"EdenFS systemd service ({service}) should be running",
            )
        else:
            raise NotImplementedError("systemd not supported on this platform")

    def assert_systemd_service_is_failed(self, eden_dir: pathlib.Path) -> None:
        if sys.platform.startswith("linux"):
            service = self.get_edenfs_systemd_service(eden_dir=eden_dir)
            assert isinstance(self, unittest.TestCase)
            self.assertEqual(
                (service.query_active_state(), service.query_sub_state()),
                ("failed", "failed"),
                f"EdenFS systemd service ({service}) should have failed",
            )
        else:
            raise NotImplementedError("systemd not supported on this platform")

    def assert_systemd_service_is_stopped(self, eden_dir: pathlib.Path) -> None:
        if sys.platform.startswith("linux"):
            service = self.get_edenfs_systemd_service(eden_dir=eden_dir)
            assert isinstance(self, unittest.TestCase)
            self.assertEqual(
                (service.query_active_state(), service.query_sub_state()),
                ("inactive", "dead"),
                f"EdenFS systemd service ({service}) should be stopped",
            )
        else:
            raise NotImplementedError("systemd not supported on this platform")


def _replicate_service_test(
    test_class: typing.Type[ServiceTestCaseBase],
) -> typing.Iterable[typing.Tuple[str, typing.Type[ServiceTestCaseBase]]]:
    tests = []

    class AdHocTest(AdHocFakeEdenFSMixin, test_class):
        pass

    tests.append(("AdHoc", typing.cast(typing.Type[ServiceTestCaseBase], AdHocTest)))

    class ManagedTest(ManagedFakeEdenFSMixin, test_class):
        pass

    tests.append(
        ("Managed", typing.cast(typing.Type[ServiceTestCaseBase], ManagedTest))
    )

    if _systemd_supported:

        class SystemdEdenCLITest(test_class, SystemdServiceTest):
            pass

        tests.append(
            (
                "SystemdEdenCLI",
                typing.cast(typing.Type[ServiceTestCaseBase], SystemdEdenCLITest),
            )
        )

    return tests


# A decorator function used to create ServiceTestCaseBase subclasses from a
# given input test class.
#
# Given an input test class named "MyTest", this will create the following
# classes which each run test a different kind of edenfs process:
#
# * MyTestAdHoc tests with ad-hoc edenfs processes [1]
# * MyTestManaged tests with 'eden start' edenfs processes (with systemd
#   integration disabled) [1]
# * MyTestSystemdEdenCLI tests with 'eden start' edenfs processes with systemd
#   integration enabled
service_test = test_replicator(_replicate_service_test)

if _systemd_supported:

    def systemd_test(
        test_class: Type[SystemdServiceTest],
    ) -> Optional[Type[SystemdServiceTest]]:
        return test_class


else:

    def systemd_test(
        test_class: Type[SystemdServiceTest],
    ) -> Optional[Type[SystemdServiceTest]]:
        # Replace the test classes with None so they won't even show up in the test
        # case listing when systemd is not supported.
        return None
