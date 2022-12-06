# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import re
from typing import Optional

from .pullrequest import PullRequestId
from .pullrequeststore import PullRequestStore


def get_pull_request_for_context(
    store: PullRequestStore,
    ctx,
) -> Optional[PullRequestId]:
    """Returns a pull request associated with a commit context, if any. Checks
    the metalog first. If not in the metalog, looks for special patterns in the
    commit message.
    """
    node = ctx.node()
    pr = store.find_pull_request(node)
    return pr if pr else _parse_github_pull_request_url(ctx.description())


def _parse_github_pull_request_url(descr: str) -> Optional[PullRequestId]:
    r"""If the commit message has a comment in a special format that indicates
    it is associated with a GitHub pull request, returns the corresponding
    PullRequestId.

    >>> descr = 'foo\nPull Request resolved: https://github.com/bolinfest/ghstack-testing/pull/71\nbar'
    >>> _parse_github_pull_request_url(descr)
    PullRequestId(hostname='github.com', owner='bolinfest', name='ghstack-testing', number=71)
    >>> _parse_github_pull_request_url('') is None
    True
    """
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
