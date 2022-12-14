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
        return FakeEdenFS.spawn_via_cli(
            eden_dir=eden_dir,
            etc_eden_dir=self.etc_eden_dir,
            home_dir=self.home_dir,
            extra_arguments=extra_arguments,
        )


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

    return tests


def _replicate_fake_service_test(
    test_class: typing.Type[ServiceTestCaseBase],
) -> typing.Iterable[typing.Tuple[str, typing.Type[ServiceTestCaseBase]]]:
    tests = []

    class ManagedTest(ManagedFakeEdenFSMixin, test_class):
        pass

    tests.append(
        ("Managed", typing.cast(typing.Type[ServiceTestCaseBase], ManagedTest))
    )

    return tests


# A decorator function used to create ServiceTestCaseBase subclasses from a
# given input test class.
#
# Given an input test class named "MyTest", this will create the following
# classes which each run test a different kind of edenfs process:
#
# * MyTestAdHoc tests with ad-hoc edenfs processes [1]
# * MyTestManaged tests with 'eden start' edenfs processes
service_test = test_replicator(_replicate_service_test)
fake_service_test = test_replicator(_replicate_fake_service_test)
