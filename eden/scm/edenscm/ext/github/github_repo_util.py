# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import re
from typing import Optional

from edenscm import git

from .graphql import GitHubPullRequest
from .pullrequeststore import PullRequestStore


def is_github_repo(repo) -> bool:
    """Create or update GitHub pull requests."""
    if not git.isgitpeer(repo):
        return False

    try:
        return repo.ui.paths.get("default", "default-push").url.host == "github.com"
    except AttributeError:  # ex. paths.default is not set
        return False


def get_pull_request_for_node(
    node: bytes,
    store: PullRequestStore,
    ctx,
) -> Optional[GitHubPullRequest]:
    """Returns a pull request associated with a commit, if any. Checks the
    metalog first. If not in the metalog, looks for special patterns in the
    commit message.
    """
    pr = store.find_pull_request(node)
    return (
        GitHubPullRequest(repo_owner=pr.owner, repo_name=pr.name, number=int(pr.number))
        if pr
        else parse_github_pull_request_url(ctx.description())
    )


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
