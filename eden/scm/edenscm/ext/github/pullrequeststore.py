# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Entry in metalog where GitHub pull request data is stored (for now).
METALOG_KEY = "github-experimental-pr-store"

import json
from typing import Dict, List, Literal, Optional, Tuple, TypedDict, Union

from edenscm import mutation
from edenscm.node import hex

from .pullrequest import PullRequestId, PullRequestIdDict

# Special type to indicate that there is explicitly no pull request associated
# with this commit and therefore its predecessors should not be consulted.
_NoAssoc = Literal["none"]

# Marker to indicate the commit has been used with `pr follow REV`.
_Follow = Literal["follow"]

_CommitEntry = Union[PullRequestIdDict, _Follow, _NoAssoc]


class _MetalogData(TypedDict):
    version: int
    commits: Dict[str, _CommitEntry]


class PullRequestStore:
    def __init__(self, repo) -> None:
        self._repo = repo

    def __str__(self) -> str:
        return json.dumps(self._get_pr_data(), indent=2)

    def map_commit_to_pull_request(self, node: bytes, pull_request: PullRequestId):
        mappings: List[Tuple[bytes, _CommitEntry]] = [(node, pull_request.as_dict())]
        self._write_mappings(mappings)

    def unlink_all(self, nodes: List[bytes]):
        mappings: List[Tuple[bytes, _CommitEntry]] = []
        for n in nodes:
            t: Tuple[bytes, _CommitEntry] = (n, "none")
            mappings.append(t)
        self._write_mappings(mappings)

    def follow_all(self, nodes: List[bytes]):
        mappings: List[Tuple[bytes, _CommitEntry]] = []
        for n in nodes:
            t: Tuple[bytes, _CommitEntry] = (n, "follow")
            mappings.append(t)
        self._write_mappings(mappings)

    def _write_mappings(
        self,
        mappings: List[Tuple[bytes, _CommitEntry]],
    ):
        pr_data = self._get_pr_data()
        commits = pr_data["commits"]
        for node, entry in mappings:
            commits[hex(node)] = entry
        with self._repo.lock(), self._repo.transaction("github"):
            ml = self._repo.metalog()
            blob = encode_pr_data(pr_data)
            ml.set(METALOG_KEY, blob)

    def is_follower(self, node: bytes) -> bool:
        entry = self._find_entry(node)
        return entry == "follow"

    def find_pull_request(self, node: bytes) -> Optional[PullRequestId]:
        entry = self._find_entry(node)
        if entry is None or isinstance(entry, str):
            return None
        else:
            return PullRequestId(
                hostname=entry.get("hostname", "github.com"),
                owner=entry["owner"],
                name=entry["name"],
                number=entry["number"],
            )

    def _find_entry(self, node: bytes) -> Optional[_CommitEntry]:
        commits = self._get_commits()
        for n in mutation.allpredecessors(self._repo, [node]):
            entry: Optional[_CommitEntry] = commits.get(hex(n))
            if isinstance(entry, str):
                assert entry == "none" or entry == "follow"
                return entry
            elif entry:
                pr: PullRequestIdDict = entry
                return pr

        return None

    def _get_pr_data(self) -> _MetalogData:
        ml = self._repo.metalog()
        blob = ml.get(METALOG_KEY)
        if blob:
            return decode_pr_data(blob)
        else:
            # Default value for METALOG_KEY.
            return {"version": 1, "commits": {}}

    def _get_commits(self) -> Dict[str, _CommitEntry]:
        pr_data = self._get_pr_data()
        return pr_data["commits"]


"""eventually, we will provide a native implementation for encoding/decoding,
but for now, we will use basic JSON encoding.
"""


def encode_pr_data(pr_data: _MetalogData) -> bytes:
    return json.dumps(pr_data).encode("utf8")


def decode_pr_data(blob: bytes) -> _MetalogData:
    metalog_data: _MetalogData = json.loads(blob)
    assert isinstance(metalog_data, dict)
    return metalog_data
