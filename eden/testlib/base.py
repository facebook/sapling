# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import os
import unittest
from pathlib import Path
from typing import Callable, TypeVar

from .config import Config
from .repo import Repo
from .server import LocalServer, MononokeServer, Server
from .util import new_dir, test_globals
from .workingcopy import WorkingCopy


class BaseTest(unittest.TestCase):
    def setUp(self) -> None:
        test_globals.setup()
        self.addCleanup(test_globals.cleanup)
        self.server = self.new_server()
        self.addCleanup(self.server.cleanup)
        self.config = Config(Path(test_globals.env["HGRCPATH"]))
        self._add_production_configs()
        self.repo = self.server.clone()

    def _add_production_configs(self) -> None:
        # Most production configs should be loaded via dynamicconfig. The ones
        # below are test-specific overrides to do things like pin timestamps,
        # set test cache directories, disable certain user-oriented output, etc.
        # data usage and f
        self.config.append(
            f"""
[commitcloud]
enablestatus = False
hostname = testhost
servicetype = local
servicelocation = {new_dir()}

[mutation]
date = 0 0

[remotefilelog]
cachepath = {new_dir()}

[edenfs]
backing-repos-dir={new_dir()}
command={str(os.getenv("EDENFSCTL_RUST_PATH"))}
"""
        )

        if os.environ.get("USE_MONONOKE", False):
            cert_dir = os.environ["HGTEST_CERTDIR"]
            self.config.append(
                f"""
[auth]
mononoke.cert={cert_dir}/client0.crt
mononoke.key={cert_dir}/client0.key
mononoke.cacerts={cert_dir}/root-ca.crt
mononoke.prefix=mononoke://*
mononoke.cn=localhost
edenapi.cert={cert_dir}/client0.crt
edenapi.key={cert_dir}/client0.key
edenapi.prefix=localhost
edenapi.schemes=https
edenapi.cacerts={cert_dir}/root-ca.crt

[web]
cacerts={cert_dir}/root-ca.crt

[edenapi]
url=https://localhost:{self.server.port}/edenapi
"""
            )

    def new_server(self) -> Server:
        if os.environ.get("USE_MONONOKE", False):
            return MononokeServer(record_stderr_to_file=test_globals.debug)
        else:
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
            wc = self.repo.new_working_copy(path=new_dir(label="Eden Dir"), eden=True)
            self.addCleanup(wc.cleanup)
        else:
            wc = self.repo.new_working_copy()
        return func(self, self.repo, wc)

    return wrapper
