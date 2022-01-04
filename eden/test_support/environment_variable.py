#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
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
