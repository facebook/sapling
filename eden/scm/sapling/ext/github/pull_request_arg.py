# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""utility function for parsing a user-supplied pull request identifier on the command line"""

import re
from typing import Optional

from .github_repo_util import find_github_repo
from .pullrequest import PullRequestId

RE_PR_URL = re.compile(
    r"^https://(?P<hostname>[^/]+)/(?P<owner>[^/]+)/(?P<name>[^/]+)/pull/(?P<number>[0-9]+)/?$"
)


def parse_pull_request_arg(arg: str, repo=None) -> Optional[PullRequestId]:
    """Uses various heuristics to determine the pull request identifed by the
    user-supplied arg.
    """
    pr = _parse_pull_request_arg_as_url(arg)
    if pr:
        return pr

    # See if arg is a number and can be resolved against repo.
    pr_num = _parse_positive_int(arg)
    if pr_num and repo:
        github_repo = find_github_repo(repo).ok()
        if github_repo:
            return PullRequestId(
                hostname=github_repo.hostname,
                owner=github_repo.owner,
                name=github_repo.name,
                number=pr_num,
            )

    return None


def _parse_pull_request_arg_as_url(arg: str) -> Optional[PullRequestId]:
    """Supports the case where the user copy/pastes the pull request URL from
    the location bar in the browser.

    >>> _parse_pull_request_arg_as_url("https://github.com/facebook/sapling/pull/321")
    PullRequestId(hostname='github.com', owner='facebook', name='sapling', number=321)
    >>> _parse_pull_request_arg_as_url("https://example.org/facebook/sapling/pull/321")
    PullRequestId(hostname='example.org', owner='facebook', name='sapling', number=321)
    """
    m = RE_PR_URL.match(arg)
    if not m:
        return None

    hostname = m.group("hostname")
    owner = m.group("owner")
    name = m.group("name")
    number = int(m.group("number"))
    return PullRequestId(hostname=hostname, owner=owner, name=name, number=number)


def _parse_positive_int(arg: str) -> Optional[int]:
    """Returns arg parsed as a positive, base-10 integer or None.

    >>> _parse_positive_int("42")
    42
    >>> _parse_positive_int("017")
    17
    >>> _parse_positive_int("0") is None
    True
    >>> _parse_positive_int("foobar") is None
    True
    """
    try:
        i = int(arg, 10)
    except ValueError:
        return None
    return i if i > 0 else None
