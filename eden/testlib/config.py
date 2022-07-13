# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import os
from pathlib import Path

from .util import trace


class Config:
    path: Path

    def __init__(self, path: Path) -> None:
        assert os.path.isabs(path), f"config path {path} is not absolute"
        self.path = path

    def add(self, section: str, key: str, value: str) -> None:
        trace(f"set config: {section}.{key}={value}")
        self.append(f"[{section}]\n{key}={value}")

    def append(self, text: str) -> None:
        with open(self.path, mode="a+") as f:
            f.write("\n" + text + "\n")

    def enable(self, extension: str) -> None:
        trace(f"enable extension: {extension}")
        self.add("extensions", extension, "")
