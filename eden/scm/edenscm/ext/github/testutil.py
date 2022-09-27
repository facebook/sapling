# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""test utilities for doctests in this folder
"""


class FakeRepo:
    pass


class FakePullRequestStore:
    def find_pull_request(self, node: bytes):
        return None


fake_pull_request_store_singleton = FakePullRequestStore()


class FakeContext:
    def __init__(self, desc: str):
        self._desc = desc

    def description(self) -> str:
        return self._desc

    def node(self) -> bytes:
        return b"\x0d\x0e\x0a\x0d\x0b\x0e\x0e\x0f" * 5


def fake_args():
    return {
        "cache": {
            "github_pr_store": fake_pull_request_store_singleton,
        },
        "revcache": {},
    }
