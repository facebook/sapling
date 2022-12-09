# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""make calls to GitHub's GraphQL API
"""

import asyncio
from typing import Dict, Iterable, List, Optional, Union

from ghstack.github import GitHubEndpoint
from ghstack.github_gh_cli import make_request

from .pullrequest import GraphQLPullRequest, PullRequestId


PULL_REQUEST_QUERY = """
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

      commits(last: 1) {
        nodes {
          commit {
            statusCheckRollup {
              state
            }
          }
        }
      }

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


def get_pull_request_data(pr: PullRequestId) -> Optional[GraphQLPullRequest]:
    params = _generate_params(pr)
    loop = asyncio.get_event_loop()
    result = loop.run_until_complete(make_request(params, hostname=pr.get_hostname()))
    if result.is_error():
        # Log error?
        return None

    pr = result.ok["data"]["repository"]["pullRequest"]
    return GraphQLPullRequest(pr)


def get_pull_request_data_list(
    github: GitHubEndpoint,
    pr_list: Iterable[PullRequestId],
) -> List[Optional[GraphQLPullRequest]]:
    requests = [
        github.graphql(PULL_REQUEST_QUERY, **_generate_params(pr)) for pr in pr_list
    ]
    loop = asyncio.get_event_loop()
    responses = loop.run_until_complete(asyncio.gather(*requests))
    result = []
    for resp in responses:
        if resp.is_error():
            result.append(None)
        else:
            pr_data = resp.ok["data"]["repository"]["pullRequest"]
            result.append(GraphQLPullRequest(pr_data))
    return result


def _generate_params(pr: PullRequestId) -> Dict[str, Union[str, int, bool]]:
    return {
        "owner": pr.owner,
        "name": pr.name,
        "number": pr.number,
    }
