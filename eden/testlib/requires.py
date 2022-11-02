# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import platform
from typing import Any, Callable, Dict, List, TypeVar

from .base import BaseTest


TestType = TypeVar("TestType")

# pyre-ignore
def require(**kwargs: Dict[str, Any]) -> Callable[[TestType], TestType]:
    has: bool = True
    missing: List[str] = []

    for name, value in kwargs.items():
        checker = checkers[name]
        if not checker(value):
            has = False
            missing.append("%s=%s" % (name, value))

    def func(cls: TestType) -> TestType:
        if not has:

            def skip(self: BaseTest) -> None:
                self.skipTest("skipping due to missing requirement(s) - %s" % missing)

            cls.setUp = skip
        return cls

    return func


def is_caseinsensitive(expected: bool) -> bool:
    actual = is_os("osx") or is_os("windows")
    return actual == expected


def is_os(name: str) -> bool:
    system = platform.system()
    if name == "osx":
        return system == "Darwin"
    if name == "linux":
        return system == "Linux"
    if name == "windows":
        return system == "Windows"
    return False


# pyre-ignore
checkers: Dict[str, Callable[[Any], bool]] = {
    "caseinsensitive": is_caseinsensitive,
    "os": is_os,
}
