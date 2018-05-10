#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import os

from .hg_extension_test_base import EdenHgTestCase


class HisteditCommand:
    """Utility to facilitate running `hg histedit` from an integration test."""

    def __init__(self):
        self._actions = []

    def pick(self, commit_hash: str) -> None:
        self._actions.append("pick %s\n" % commit_hash)

    def roll(self, commit_hash: str) -> None:
        self._actions.append("roll %s\n" % commit_hash)

    def drop(self, commit_hash: str) -> None:
        self._actions.append("drop %s\n" % commit_hash)

    def stop(self, commit_hash: str) -> None:
        self._actions.append("stop %s\n" % commit_hash)

    def run(self, test_base: EdenHgTestCase, ancestor: str = None) -> None:
        commands_file = os.path.join(test_base.tmp_dir, "histedit_commands.txt")
        with open(commands_file, "w") as f:
            [f.write(action) for action in self._actions]

        args = ["histedit", "--commands", commands_file]
        if ancestor is not None:
            args.append(ancestor)
        test_base.hg(*args)
