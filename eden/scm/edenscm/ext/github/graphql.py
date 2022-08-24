# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""make calls to GitHub's GraphQL API
"""

import configparser
import os
from dataclasses import dataclass
from pathlib import Path
from typing import Optional

from bindings import github
from edenscm import pycompat


@dataclass
class GitHubPullRequest:
    # In GitHub, a "RepositoryOwner" is either an "Organization" or a "User":
    # https://docs.github.com/en/graphql/reference/interfaces#repositoryowner
    repo_owner: str
    repo_name: str
    number: int

    def as_url(self, domain=None) -> str:
        domain = domain or "github.com"
        return f"https://{domain}/{self.repo_owner}/{self.repo_name}/pull/{self.number}"


def get_pull_request_data(token: str, pr: GitHubPullRequest):
    return github.get_pull_request(token, pr.repo_owner, pr.repo_name, pr.number)


def get_github_oauth_token() -> Optional[str]:
    # For now, we only support reading the OAuth token from .ghstackrc.
    # This is a simplified version of the logic ghstack uses to read its own
    # config file:
    # https://github.com/ezyang/ghstack/blob/master/ghstack/config.py
    current_dir = Path(pycompat.getcwd())

    while current_dir != Path("/"):
        config_path = "/".join([str(current_dir), ".ghstackrc"])
        token = try_parse_oauth_token_from_ghstackrc(config_path)
        if token:
            return token
        current_dir = current_dir.parent

    # If this is used in a /tmp folder, then ~/.ghstackrc will not be an
    # ancestor of getcwd(), but it should be considered, anyway.
    config_path = os.path.expanduser("~/.ghstackrc")
    return try_parse_oauth_token_from_ghstackrc(config_path)


def try_parse_oauth_token_from_ghstackrc(config_path: str) -> Optional[str]:
    config = configparser.ConfigParser()
    try:
        with open(config_path) as f:
            config.read_file(f)
            token = config.get("ghstack", "github_oauth")
            if token:
                return token
    except Exception:
        # Could be FileNotFoundError, a parse error...just ignore.
        pass
    return None
