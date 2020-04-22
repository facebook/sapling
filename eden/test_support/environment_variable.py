#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import abc
import contextlib
import os
from typing import Any, Callable, Generator, Mapping, Optional


def _setenv(name: str, value: Optional[str]) -> None:
    if value is None:
        os.environ.pop(name, None)
    else:
        os.environ[name] = value


@contextlib.contextmanager
def setenv_scope(name: str, value: Optional[str]) -> Generator[None, None, None]:
    old_value = os.environ.get(name)
    _setenv(name, value)
    yield
    _setenv(name, old_value)


@contextlib.contextmanager
def unsetenv_scope(name: str) -> Generator[None, None, None]:
    old_value = os.environ.get(name)
    os.environ.pop(name, None)
    yield
    _setenv(name, old_value)


class EnvironmentVariableMixin(metaclass=abc.ABCMeta):
    def set_environment_variable(self, name: str, value: str) -> None:
        self.__add_cleanup_for_environment_variable(name)
        os.environ[name] = value

    def set_environment_variables(self, variables: Mapping[str, str]) -> None:
        for name, value in variables.items():
            self.set_environment_variable(name, value)

    def unset_environment_variable(self, name: str) -> None:
        self.__add_cleanup_for_environment_variable(name)
        os.environ.pop(name, None)

    def __add_cleanup_for_environment_variable(self, name: str) -> None:
        old_value = os.environ.get(name)
        self.addCleanup(_setenv, name, old_value)

    def addCleanup(
        self, function: Callable[..., Any], *args: Any, **kwargs: Any
    ) -> None:
        raise NotImplementedError()
