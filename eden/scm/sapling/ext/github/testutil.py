# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""test utilities for doctests in this folder
"""

from sapling import git


class FakeRepo:
    pass


"""Designed to be compatible with find_github_repo()."""
class FakeGitHubRepo:
    def __init__(
        self,
        hostname: str = "github.com",
        owner: str = "facebook",
        name: str = "sapling",
    ) -> None:
        # This should be sufficient to satisfy git.isgitpeer().
        self.storerequirements = {git.GIT_STORE_REQUIREMENT}
        self.github_url = f"https://{hostname}/{owner}/{name}"

    def get_github_url(self) -> str:
        return self.github_url


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
