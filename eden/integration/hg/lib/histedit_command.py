#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
from typing import Optional

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

    def run(self, test_base: EdenHgTestCase, ancestor: Optional[str] = None) -> None:
        commands_file = os.path.join(test_base.tmp_dir, "histedit_commands.txt")
        with open(commands_file, "w") as f:
            [f.write(action) for action in self._actions]

        args = ["histedit", "--commands", commands_file]
        if ancestor is not None:
            args.append(ancestor)
        test_base.hg(*args)
