# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Entry in metalog where GitHub pull request data is stored (for now).
METALOG_KEY = "github-experimental-pr-store"

import json
from typing import Dict, Literal, Optional, TypedDict, Union

from edenscm import mutation
from edenscm.node import hex

from .pullrequest import PullRequestId, PullRequestIdDict

# Special type to indicate that there is explicitly no pull request associated
# with this commit and therefore its predecessors should not be consulted.
_NoAssoc = Literal["none"]

_CommitEntry = Union[PullRequestIdDict, _NoAssoc]


class _MetalogData(TypedDict):
    version: int
    commits: Dict[str, _CommitEntry]


class PullRequestStore:
    def __init__(self, repo) -> None:
        self._repo = repo

    def __str__(self) -> str:
        return json.dumps(self._get_pr_data(), indent=2)

    def map_commit_to_pull_request(self, node: bytes, pull_request: PullRequestId):
        self._write_mapping(node, pull_request.as_dict())

    def unlink(self, node: bytes):
        self._write_mapping(node, "none")

    def _write_mapping(self, node: bytes, json_serializable_value: _CommitEntry):
        pr_data = self._get_pr_data()
        commits = pr_data["commits"]
        commits[hex(node)] = json_serializable_value
        with self._repo.lock(), self._repo.transaction("github"):
            ml = self._repo.metalog()
            blob = encode_pr_data(pr_data)
            ml.set(METALOG_KEY, blob)

    def find_pull_request(self, node: bytes) -> Optional[PullRequestId]:
        commits = self._get_commits()
        for n in mutation.allpredecessors(self._repo, [node]):
            entry: Optional[_CommitEntry] = commits.get(hex(n))
            if isinstance(entry, str):
                assert entry == "none"
                return None
            elif entry:
                pr: PullRequestIdDict = entry
                return PullRequestId(
                    owner=pr["owner"], name=pr["name"], number=pr["number"]
                )
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
