# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict
from __future__ import annotations

import json
from typing import List


class Status:
    added: List[str]
    deleted: List[str]
    modified: List[str]
    removed: List[str]
    untracked: List[str]

    def __init__(self, raw_json: str) -> None:
        self.added = []
        self.deleted = []
        self.modified = []
        self.removed = []
        self.untracked = []

        for entry in json.loads(raw_json):
            st = entry["status"]
            path = entry["path"]
            if st == "M":
                self.modified.append(path)
            elif st == "A":
                self.added.append(path)
            elif st == "R":
                self.removed.append(path)
            elif st == "!":
                self.deleted.append(path)
            elif st == "?":
                self.untracked.append(path)
            else:
                raise ValueError("unknown status state '%s' for '%s'" % (st, path))

    def empty(self) -> bool:
        return not (
            self.added + self.deleted + self.modified + self.removed + self.untracked
        )
