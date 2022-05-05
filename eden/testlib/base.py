# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import os
import unittest
from pathlib import Path
from typing import Callable, TypeVar

from .repo import Repo
from .server import LocalServer, Server
from .util import new_dir, test_globals
from .workingcopy import WorkingCopy


class BaseTest(unittest.TestCase):
    def setUp(self) -> None:
        test_globals.setup()
        self.addCleanup(test_globals.cleanup)
        self.server = self.new_server()
        self.repo = self.server.clone()
        self._add_production_configs(Path(test_globals.env["HGRCPATH"]))

    def _add_production_configs(self, hgrc: Path) -> None:
        with open(hgrc, "w") as f:
            f.write(
                f"""
[commitcloud]
enablestatus = False
hostname = testhost
remotebookmarkssync = True
servicetype = local
servicelocation = {new_dir()}
token_enforced = False

[experimental]
changegroup3 = True
evolution = obsolete
narrow-heads = True

[extensions]
amend =
commitcloud =
infinitepush =
rebase =
remotefilelog =
remotenames =
smartlog =
treemanifest =

[mutation]
date = "0 0"
enabled = True
record = True

[remotefilelog]
cachepath = {new_dir()}
http = True

[remotenames]
hoist = remote
rename.default = remote
selectivepull = True
selectivepulldefault = master

[treemanifest]
http = True

[visibility]
enabled = True
"""
            )

    def new_server(self) -> Server:
        return LocalServer()


# Contravariance rules in pyre mean we can't specify a base type directly as an
# argument to callable. Instead we need to use generics. See:
# - https://pyre-check.org/docs/errors/#contravariance
# - https://fb.workplace.com/groups/pyreqa/posts/4333828650040267/?comment_id=4335743286515470
TBase = TypeVar("TBase", bound=BaseTest)


def hgtest(func: Callable[[TBase, Repo, WorkingCopy], None]) -> Callable[[TBase], None]:
    def wrapper(self: TBase) -> None:
        use_eden = os.environ.get("USE_EDEN", False)
        if use_eden:
            wc = self.repo.new_working_copy(path=new_dir(), eden=True)
            self.addCleanup(wc.cleanup)
        else:
            wc = self.repo.new_working_copy()
        return func(self, self.repo, wc)

    return wrapper
