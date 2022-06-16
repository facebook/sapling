# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

from typing import Dict


class RepoGenerator:
    _commits: int
    _files: int

    def __init__(self) -> None:
        self._commits = 0
        self._files = 0

    def gen_commit_data(self) -> Dict[str, str]:
        self._commits += 1
        return {
            "message": f"message{self._commits}",
            "date": "1970-01-01 UTC",
        }

    def gen_file_name(self) -> str:
        self._files += 1
        return f"file{self._files}"
