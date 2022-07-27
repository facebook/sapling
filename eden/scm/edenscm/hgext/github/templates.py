# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""templates to use when the GitHub extension is enabled
"""

import re
from dataclasses import dataclass
from typing import Optional


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


def github_pull_request_url(descr: str, domain=None) -> Optional[str]:
    r"""Returns the pull request associated with a commit based on the contents
    of the commit message, if appropriate.

    >>> descr = 'foo\nPull Request resolved: https://github.com/bolinfest/ghstack-testing/pull/71\nbar'
    >>> github_pull_request_url(descr)
    'https://github.com/bolinfest/ghstack-testing/pull/71'
    """
    pull_request = parse_github_pull_request_url(descr)
    return pull_request.as_url(domain) if pull_request else None


def github_pull_request_repo_owner(descr: str) -> Optional[str]:
    r"""Returns the repo owner for a pull request associated with a commit based
    on the contents of the commit message, if appropriate.
    >>> descr = 'foo\nPull Request resolved: https://github.com/bolinfest/ghstack-testing/pull/71\nbar'
    >>> github_pull_request_repo_owner(descr)
    'bolinfest'
    """
    pull_request = parse_github_pull_request_url(descr)
    return pull_request.repo_owner if pull_request else None


def github_pull_request_repo_name(descr: str) -> Optional[str]:
    r"""Returns the repo name for a pull request associated with a commit based
    on the contents of the commit message, if appropriate.
    >>> descr = 'foo\nPull Request resolved: https://github.com/bolinfest/ghstack-testing/pull/71\nbar'
    >>> github_pull_request_repo_name(descr)
    'ghstack-testing'
    """
    pull_request = parse_github_pull_request_url(descr)
    return pull_request.repo_name if pull_request else None


def github_pull_request_number(descr: str) -> Optional[int]:
    r"""Returns the number for a pull request associated with a commit based
    on the contents of the commit message, if appropriate.
    >>> descr = 'foo\nPull Request resolved: https://github.com/bolinfest/ghstack-testing/pull/71\nbar'
    >>> github_pull_request_number(descr)
    71
    """
    pull_request = parse_github_pull_request_url(descr)
    return pull_request.number if pull_request else None


def parse_github_pull_request_url(descr: str) -> Optional[GitHubPullRequest]:
    r"""If the commit message has a comment in a special format that indicates
    it is associated with a GitHub pull request, returns the corresponding
    GitHubPullRequest.

    >>> descr = 'foo\nPull Request resolved: https://github.com/bolinfest/ghstack-testing/pull/71\nbar'
    >>> parse_github_pull_request_url(descr)
    GitHubPullRequest(repo_owner='bolinfest', repo_name='ghstack-testing', number=71)
    >>> parse_github_pull_request_url('') is None
    True
    """
    # This is the format used by ghstack, though other variants may be supported
    # in the future.
    match = re.search(
        r"^Pull Request resolved: https://github.com/([^/]*)/([^/]*)/pull/([1-9][0-9]*)$",
        descr,
        re.MULTILINE,
    )
    if not match:
        return None
    owner, name, number = match.groups()
    return GitHubPullRequest(repo_owner=owner, repo_name=name, number=int(number))
