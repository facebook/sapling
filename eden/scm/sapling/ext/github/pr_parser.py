# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import re
from typing import Optional

from .github_repo_util import find_github_repo
from .pullrequest import PullRequestId
from .pullrequeststore import PullRequestStore


def get_pull_request_for_context(
    store: PullRequestStore,
    repo,
    ctx,
) -> Optional[PullRequestId]:
    """Returns a pull request associated with a commit context, if any. Checks
    the metalog first. If not in the metalog, looks for special patterns in the
    commit message.
    """
    node = ctx.node()
    pr = store.find_pull_request(node)
    return pr if pr else _parse_github_pull_request_url(ctx.description(), repo)


def _parse_github_pull_request_url(descr: str, repo) -> Optional[PullRequestId]:
    r"""If the commit message has a comment in a special format that indicates
    it is associated with a GitHub pull request, returns the corresponding
    PullRequestId.

    Match the number at the end of the title.
    >>> from .testutil import FakeGitHubRepo
    >>> gh_repo = FakeGitHubRepo(name="dotslash")
    >>> commit_msg = 'Document experimental `fetch` subcommand (#24)\nSummary:\nDocument...'
    >>> _parse_github_pull_request_url(commit_msg, gh_repo)
    PullRequestId(hostname='github.com', owner='facebook', name='dotslash', number=24)

    Should work with a single line title.
    >>> commit_msg = 'Document experimental `fetch` subcommand (#24)'
    >>> _parse_github_pull_request_url(commit_msg, gh_repo)
    PullRequestId(hostname='github.com', owner='facebook', name='dotslash', number=24)

    Does not match if the pattern appears after the first line.
    >>> commit_msg = 'TITLE\nDocument experimental `fetch` subcommand (#24)\nSummary:\nDocument...'
    >>> _parse_github_pull_request_url(commit_msg, gh_repo) is None
    True

    Match the "Pull Request resolved" text.
    >>> descr = 'foo\nPull Request resolved: https://github.com/bolinfest/ghstack-testing/pull/71\nbar'
    >>> _parse_github_pull_request_url(descr, gh_repo)
    PullRequestId(hostname='github.com', owner='bolinfest', name='ghstack-testing', number=71)

    Test a trivial "no match" case.
    >>> _parse_github_pull_request_url('', gh_repo) is None
    True
    """
    # The default for "squash and merge" in the GitHub pull request flow appears
    # to put the pull request number at the end of the first line of the commit
    # message, so check that first.
    match = re.search(r'^.*\(#([1-9][0-9]*)\)\r?(\n|$)', descr)
    if match:
        result = find_github_repo(repo)
        if result.is_ok():
            github_repo = result.unwrap()
            return PullRequestId(
                hostname=github_repo.hostname,
                owner=github_repo.owner,
                name=github_repo.name,
                number=int(match.group(1)),
            )

    # This is the format used by ghstack, though other variants may be supported
    # in the future.
    match = re.search(
        r"^Pull Request resolved: https://([^/]*)/([^/]*)/([^/]*)/pull/([1-9][0-9]*)$",
        descr,
        re.MULTILINE,
    )
    if not match:
        return None
    hostname, owner, name, number = match.groups()
    return PullRequestId(hostname=hostname, owner=owner, name=name, number=int(number))
