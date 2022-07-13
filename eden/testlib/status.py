# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict
from __future__ import annotations

import json
from typing import Dict, List, Optional


class Status:
    added: List[str]
    deleted: List[str]
    modified: List[str]
    removed: List[str]
    untracked: List[str]
    copies: Dict[str, str]

    ADDED = "added"
    DELETED = "deleted"
    MODIFIED = "modified"
    REMOVED = "removed"
    UNTRACKED = "untracked"

    def __init__(self, raw_json: str) -> None:
        self.added = []
        self.deleted = []
        self.modified = []
        self.removed = []
        self.untracked = []
        self.copies = {}

        for entry in json.loads(raw_json):
            st = entry["status"]
            path = entry["path"]
            copy = entry.get("copy")
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

            if copy is not None:
                self.copies[path] = copy

    def __getitem__(self, path: str) -> Optional[str]:
        for kind, entries in [
            (Status.ADDED, self.added),
            (Status.DELETED, self.deleted),
            (Status.MODIFIED, self.modified),
            (Status.REMOVED, self.removed),
            (Status.UNTRACKED, self.untracked),
        ]:
            if path in entries:
                return kind

        return None

    def empty(self) -> bool:
        return not (
            self.added + self.deleted + self.modified + self.removed + self.untracked
        )

    def __str__(self) -> str:
        return "\n".join(
            f"{k}: {v}" for (k, v) in sorted(vars(self).items()) if len(v) > 0
        )
