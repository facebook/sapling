# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import unittest
from typing import Callable, TypeVar

from .repo import Repo
from .server import LocalServer, Server
from .util import test_globals
from .workingcopy import WorkingCopy


class BaseTest(unittest.TestCase):
    def setUp(self) -> None:
        test_globals.setup()
        self.server = self.new_server()
        self.repo = self.server.clone()

    def tearDown(self) -> None:
        test_globals.cleanup()

    def new_server(self) -> Server:
        return LocalServer()


# Contravariance rules in pyre mean we can't specify a base type directly as an
# argument to callable. Instead we need to use generics. See:
# - https://pyre-check.org/docs/errors/#contravariance
# - https://fb.workplace.com/groups/pyreqa/posts/4333828650040267/?comment_id=4335743286515470
TBase = TypeVar("TBase", bound=BaseTest)


def hgtest(func: Callable[[TBase, Repo, WorkingCopy], None]) -> Callable[[TBase], None]:
    def wrapper(self: TBase) -> None:
        func(self, self.repo, self.repo.working_copy())

    return wrapper
