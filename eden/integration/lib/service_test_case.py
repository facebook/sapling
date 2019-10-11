#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import abc
import pathlib
import typing
import unittest

from eden.test_support.environment_variable import EnvironmentVariableMixin
from eden.test_support.temporary_directory import TemporaryDirectoryMixin

from .edenfs_systemd import EdenFSSystemdMixin
from .fake_edenfs import FakeEdenFS
from .systemd import SystemdUserServiceManagerMixin
from .testcase import test_replicator


class ServiceTestCaseBase(
    unittest.TestCase,
    EnvironmentVariableMixin,
    TemporaryDirectoryMixin,
    metaclass=abc.ABCMeta,
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

    def skip_if_systemd(self, message: str) -> None:
        pass

    def get_required_eden_cli_args(self) -> typing.List[str]:
        return [
            "--etc-eden-dir",
            str(self.etc_eden_dir),
            "--home-dir",
            str(self.home_dir),
        ]

    @property
    def tmp_dir(self) -> pathlib.Path:
        if self.__tmp_dir is None:
            self.__tmp_dir = pathlib.Path(self.make_temporary_directory())
        return self.__tmp_dir

    @property
    def etc_eden_dir(self) -> pathlib.Path:
        if self.__etc_eden_dir is None:
            self.__etc_eden_dir = self.tmp_dir / "etc_eden"
            self.__etc_eden_dir.mkdir()
        return self.__etc_eden_dir

    @property
    def home_dir(self) -> pathlib.Path:
        if self.__home_dir is None:
            self.__home_dir = self.tmp_dir / "home"
            self.__home_dir.mkdir()
        return self.__home_dir


# pyre-fixme[44]: `ServiceTestCaseMixinBase` non-abstract class with abstract methods.
class ServiceTestCaseMixinBase:
    if typing.TYPE_CHECKING:

        @property
        @abc.abstractmethod
        def etc_eden_dir(self) -> pathlib.Path:
            raise NotImplementedError()

        @property
        @abc.abstractmethod
        def home_dir(self) -> pathlib.Path:
            raise NotImplementedError()


if typing.TYPE_CHECKING:

    # pyre-fixme[38]: `SystemdServiceTestCaseMarker` does not implement all
    #  inherited abstract methods.
    class SystemdServiceTestCaseMarker(EdenFSSystemdMixin):
        pass


else:

    class SystemdServiceTestCaseMarker:
        """Marker base class for tests requiring systemd integration.

        Only use SystemdServiceTestCaseMarker with @service_test.

        Subclasses can use any method in EdenFSSystemdMixin.
        """


class AdHocFakeEdenFSMixin(ServiceTestCaseMixinBase):
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


class ManagedFakeEdenFSMixin(ServiceTestCaseMixinBase):
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


# pyre-fixme[44]: `SystemdEdenCLIFakeEdenFSMixin` non-abstract class with abstract
#  methods.
class SystemdEdenCLIFakeEdenFSMixin(ServiceTestCaseMixinBase):
    """Test by using 'eden start' with systemd enabled to spawn fake_edenfs.

    Use the @service_test decorator to use this mixin automatically.
    """

    def setUp(self) -> None:
        super().setUp()  # type: ignore
        # TODO(T33122320): Don't set EDEN_EXPERIMENTAL_SYSTEMD when using
        # systemd is the default option.
        self.set_environment_variable("EDEN_EXPERIMENTAL_SYSTEMD", "1")
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

    def skip_if_systemd(self, message: str) -> None:
        self.skipTest(message)

    if typing.TYPE_CHECKING:

        @abc.abstractmethod
        def set_environment_variable(self, name: str, value: str) -> None:
            raise NotImplementedError()

        @abc.abstractmethod
        def set_up_edenfs_systemd_service(self) -> None:
            raise NotImplementedError()

        @abc.abstractmethod
        def skipTest(self, reason: typing.Any) -> None:
            raise NotImplementedError()


def _replicate_service_test(
    test_class: typing.Type[ServiceTestCaseBase], skip_systemd: bool = False
) -> typing.Iterable[typing.Tuple[str, typing.Type[ServiceTestCaseBase]]]:
    only_systemd = issubclass(test_class, SystemdServiceTestCaseMarker)
    assert not (only_systemd and skip_systemd)

    tests = []

    if not only_systemd:

        class AdHocTest(AdHocFakeEdenFSMixin, test_class):  # type: ignore
            pass

        tests.append(
            ("AdHoc", typing.cast(typing.Type[ServiceTestCaseBase], AdHocTest))
        )

        class ManagedTest(ManagedFakeEdenFSMixin, test_class):  # type: ignore
            pass

        tests.append(
            ("Managed", typing.cast(typing.Type[ServiceTestCaseBase], ManagedTest))
        )

    if not skip_systemd:

        # pyre-fixme[38]: `SystemdEdenCLITest` does not implement all inherited
        #  abstract methods.
        class SystemdEdenCLITest(
            SystemdEdenCLIFakeEdenFSMixin,
            test_class,  # type: ignore
            SystemdUserServiceManagerMixin,
            EdenFSSystemdMixin,
        ):
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
#
# [1] This class is *not* created if the input test class derives from
#     SystemdServiceTestCaseMarker.
service_test = test_replicator(_replicate_service_test)
