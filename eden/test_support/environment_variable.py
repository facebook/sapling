#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import abc
import os
import typing


class EnvironmentVariableMixin(metaclass=abc.ABCMeta):
    def set_environment_variable(self, name: str, value: str) -> None:
        self.__add_cleanup_for_environment_variable(name)
        os.environ[name] = value

    def set_environment_variables(self, variables: typing.Mapping[str, str]) -> None:
        for name, value in variables.items():
            self.set_environment_variable(name, value)

    def unset_environment_variable(self, name: str) -> None:
        self.__add_cleanup_for_environment_variable(name)
        del os.environ[name]

    def __add_cleanup_for_environment_variable(self, name: str) -> None:
        old_value = os.getenv(name)

        def restore() -> None:
            if old_value is None:
                del os.environ[name]
            else:
                os.environ[name] = old_value

        self.addCleanup(restore)

    def addCleanup(
        self,
        function: typing.Callable[..., typing.Any],
        *args: typing.Any,
        **kwargs: typing.Any
    ) -> None:
        raise NotImplementedError()
