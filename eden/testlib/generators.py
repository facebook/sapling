# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict


class RepoGenerator:
    _files: int

    def __init__(self) -> None:
        self._files = 0

    def gen_file_name(self) -> str:
        self._files += 1
        return f"file{self._files}"
