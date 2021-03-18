#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

from typing import Optional, Dict, List

from .lib import testcase


@testcase.eden_repo_test
class NfsTest(testcase.EdenRepoTest):
    def populate_repo(self) -> None:
        self.repo.write_file("hello", "hola\n")
        self.repo.commit("Initial commit.")

    def edenfs_logging_settings(self) -> Optional[Dict[str, str]]:
        return {"eden.fs.nfs": "DBG7", "eden.strace": "DBG7"}

    def edenfs_extra_config(self) -> Optional[Dict[str, List[str]]]:
        return {"experimental": ["enable-nfs-server = true"]}

    def test_clone(self) -> None:
        clone_dir = self.make_temporary_directory()
        self.eden.clone(self.repo.path, clone_dir, nfs=True)
