# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Entry in metalog where GitHub pull request data is stored (for now).
METALOG_KEY = "github-experimental-pr-store"

ML_VERSION_PROPERTY = "version"
ML_COMMITS_PROPERTY = "commits"

import json
from typing import Optional

from edenscm import mutation
from edenscm.node import hex

from .pullrequest import PullRequestId

# Special value to indicate that there is explicitly no pull request associated
# with this commit and therefore its predecessors should not be consulted.
NO_ASSOC = "none"


class PullRequestStore:
    def __init__(self, repo) -> None:
        self._repo = repo

    def __str__(self):
        return json.dumps(self._get_pr_data(), indent=2)

    def map_commit_to_pull_request(self, node, pull_request: PullRequestId):
        self._write_mapping(node, pull_request.as_dict())

    def unlink(self, node):
        self._write_mapping(node, NO_ASSOC)

    def _write_mapping(self, node, json_serializable_value):
        pr_data = self._get_pr_data()
        commits = pr_data[ML_COMMITS_PROPERTY]
        commits[hex(node)] = json_serializable_value
        with self._repo.lock(), self._repo.transaction("github"):
            ml = self._repo.metalog()
            blob = encode_pr_data(pr_data)
            ml.set(METALOG_KEY, blob)

    def find_pull_request(self, node) -> Optional[PullRequestId]:
        commits = self._get_commits()
        for n in mutation.allpredecessors(self._repo, [node]):
            pr = commits.get(hex(n))
            if pr == NO_ASSOC:
                return None
            elif pr:
                return PullRequestId(
                    owner=pr["owner"], name=pr["name"], number=pr["number"]
                )
        return None

    def _get_pr_data(self):
        ml = self._repo.metalog()
        blob = ml.get(METALOG_KEY)
        if blob:
            return decode_pr_data(blob)
        else:
            # Default value for METALOG_KEY.
            return {ML_VERSION_PROPERTY: 1, ML_COMMITS_PROPERTY: {}}

    def _get_commits(self):
        pr_data = self._get_pr_data()
        return pr_data[ML_COMMITS_PROPERTY]


"""eventually, we will provide a native implementation for encoding/decoding,
but for now, we will use basic JSON encoding.
"""


def encode_pr_data(pr_data: dict) -> bytes:
    return json.dumps(pr_data).encode("utf8")


def decode_pr_data(blob: bytes) -> dict:
    blob = json.loads(blob)
    assert isinstance(blob, dict)
    return blob
