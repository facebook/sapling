# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""make calls to GitHub's GraphQL API
"""

import asyncio
from typing import Dict, Optional, Union

from ghstack.github_gh_cli import make_request

from .pullrequest import GraphQLPullRequest, PullRequestId


def get_pull_request_data(pr: PullRequestId) -> Optional[GraphQLPullRequest]:
    query = """
query PullRequestQuery($owner: String!, $name: String!, $number: Int!) {
  repository(name: $name, owner: $owner) {
    pullRequest(number: $number) {
      id
      number
      url
      title
      body

      isDraft
      state
      closed
      merged
      reviewDecision

      baseRefName
      baseRefOid
      baseRepository {
        nameWithOwner
      }
      headRefName
      headRefOid
      headRepository {
        nameWithOwner
      }
    }
  }
}
"""
    params: Dict[str, Union[str, int, bool]] = {
        "query": query,
        "owner": pr.owner,
        "name": pr.name,
        "number": pr.number,
    }
    loop = asyncio.get_event_loop()
    result = loop.run_until_complete(make_request(params))
    if result.is_error():
        # Log error?
        return None

    pr = result.ok["data"]["repository"]["pullRequest"]
    return GraphQLPullRequest(pr)
