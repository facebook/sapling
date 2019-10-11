#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

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
        self.__unset_environment_variable_with_cleanup(name)

    def __add_cleanup_for_environment_variable(self, name: str) -> None:
        old_value = os.getenv(name)

        def restore() -> None:
            if old_value is None:
                self.__unset_environment_variable_with_cleanup(name)
            else:
                os.environ[name] = old_value

        self.addCleanup(restore)

    def __unset_environment_variable_with_cleanup(self, name: str) -> None:
        try:
            del os.environ[name]
        except KeyError:
            pass

    def addCleanup(
        self,
        function: typing.Callable[..., typing.Any],
        *args: typing.Any,
        **kwargs: typing.Any
    ) -> None:
        raise NotImplementedError()
