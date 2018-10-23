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

from .fake_edenfs import FakeEdenFS
from .testcase import test_replicator


class ServiceTestCaseBase(unittest.TestCase, metaclass=abc.ABCMeta):
    """Abstract base class for tests covering 'eden start', 'eden stop', etc.

    Use the @service_test decorator to make a concrete subclass.
    """

    @abc.abstractmethod
    def spawn_fake_edenfs(
        self, eden_dir: pathlib.Path, extra_arguments: typing.Sequence[str] = ()
    ) -> FakeEdenFS:
        raise NotImplementedError()


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
        return FakeEdenFS.spawn_via_cli(
            eden_dir=eden_dir, extra_arguments=extra_arguments
        )


def _replicate_service_test(
    test_class: typing.Type[ServiceTestCaseBase]
) -> typing.Iterable[typing.Tuple[str, typing.Type[ServiceTestCaseBase]]]:
    class ManagedTest(ManagedFakeEdenFSMixin, test_class):
        pass

    class AdHocTest(AdHocFakeEdenFSMixin, test_class):
        pass

    return [("Managed", ManagedTest), ("AdHoc", AdHocTest)]


# A decorator function used to create ServiceTestCaseBase subclasses from a
# given input test class.
#
# Given an input test class named "MyTest", this will create two separate
# classes named "MyTestAdHoc" and "MyTestManaged", which run the tests with
# ad-hoc and managed edenfs processes, respectively.
service_test = test_replicator(_replicate_service_test)
