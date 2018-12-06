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

from .edenfs_systemd import EdenFSSystemdMixin
from .environment_variable import EnvironmentVariableMixin
from .fake_edenfs import FakeEdenFS
from .systemd import SystemdUserServiceManagerMixin
from .testcase import test_replicator


class ServiceTestCaseBase(
    unittest.TestCase, EnvironmentVariableMixin, metaclass=abc.ABCMeta
):
    """Abstract base class for tests covering 'eden start', 'eden stop', etc.

    Use the @service_test decorator to make a concrete subclass.
    """

    @abc.abstractmethod
    def spawn_fake_edenfs(
        self, eden_dir: pathlib.Path, extra_arguments: typing.Sequence[str] = ()
    ) -> FakeEdenFS:
        raise NotImplementedError()

    def skip_if_systemd(self, message: str) -> None:
        pass


class AdHocFakeEdenFSMixin:
    """Test by spawning fake_edenfs directly.

    Use the @service_test decorator to use this mixin automatically.
    """

    def spawn_fake_edenfs(
        self, eden_dir: pathlib.Path, extra_arguments: typing.Sequence[str] = ()
    ) -> FakeEdenFS:
        return FakeEdenFS.spawn(eden_dir=eden_dir, extra_arguments=extra_arguments)


class ManagedFakeEdenFSMixin:
    """Test by using 'eden start' to spawn fake_edenfs.

    Use the @service_test decorator to use this mixin automatically.
    """

    def spawn_fake_edenfs(
        self, eden_dir: pathlib.Path, extra_arguments: typing.Sequence[str] = ()
    ) -> FakeEdenFS:
        # TODO(T33122320): Opt out of using systemd when using systemd is the
        # default option.
        return FakeEdenFS.spawn_via_cli(
            eden_dir=eden_dir, extra_arguments=extra_arguments
        )


class SystemdEdenCLIFakeEdenFSMixin:
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
        self, eden_dir: pathlib.Path, extra_arguments: typing.Sequence[str] = ()
    ) -> FakeEdenFS:
        return FakeEdenFS.spawn_via_cli(
            eden_dir=eden_dir, extra_arguments=extra_arguments
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
    tests = []

    class AdHocTest(AdHocFakeEdenFSMixin, test_class):  # type: ignore
        pass

    tests.append(("AdHoc", typing.cast(typing.Type[ServiceTestCaseBase], AdHocTest)))

    class ManagedTest(ManagedFakeEdenFSMixin, test_class):  # type: ignore
        pass

    tests.append(
        ("Managed", typing.cast(typing.Type[ServiceTestCaseBase], ManagedTest))
    )

    if not skip_systemd:

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
# * MyTestAdHoc tests with ad-hoc edenfs processes
# * MyTestManaged tests with 'eden start' edenfs processes (with systemd
#   integration disabled)
# * MyTestSystemdEdenCLI tests with 'eden start' edenfs processes with systemd
#   integration enabled
service_test = test_replicator(_replicate_service_test)
