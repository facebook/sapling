# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""logic for submit.py implemented by shelling out to the GitHub CLI.

Ultimately, we expect to replace this with a Rust implementation that makes
the API calls directly so we can (1) avoid spawning so many processes, and
(2) do more work in parallel.
"""

from dataclasses import dataclass
from typing import Dict, Optional, Tuple, Union

from ghstack.github_gh_cli import make_request, Result

from .pullrequest import PullRequestId

_Params = Union[str, int, bool]


@dataclass
class Repository:
    # ID for the repository for use with other GitHub API calls.
    id: str
    # If GitHub Enterprise, this is the Enterprise hostname; otherwise, it is
    # "github.com".
    hostname: str
    # In GitHub, a "RepositoryOwner" is either an "Organization" or a "User":
    # https://docs.github.com/en/graphql/reference/interfaces#repositoryowner
    owner: str
    # Name of the GitHub repo within the organization.
    name: str
    # Name of the default branch.
    default_branch: str
    # True if this is a fork.
    is_fork: bool
    # Should be set if is_fork is True, though if this is a fork of a fork,
    # then we only traverse one link in the chain, so this could still be None.
    upstream: Optional["Repository"] = None

    def get_base_branch(self) -> str:
        """If this is a fork, returns the default_branch of the upstream repo."""
        if self.upstream:
            return self.upstream.default_branch
        else:
            return self.default_branch

    def get_upstream_owner_and_name(self) -> Tuple[str, str]:
        """owner and name to use when creating a pull request"""
        if self.upstream:
            return (self.upstream.owner, self.upstream.name)
        else:
            return (self.owner, self.name)


async def get_repository(hostname: str, owner: str, name: str) -> Result[Repository]:
    """Returns an "ID!" for the repository that is necessary in other
    GitHub API calls.
    """
    query = """
query ($owner: String!, $name: String!) {
  repository(name: $name, owner: $owner) {
    id
    owner {
      id
      login
    }
    name
    isFork
    defaultBranchRef {
      name
    }
    parent {
      id
      owner {
        id
        login
      }
      name
      isFork
      defaultBranchRef {
        name
      }
    }
  }
}
"""
    params: Dict[str, _Params] = {"query": query, "owner": owner, "name": name}
    result = await make_request(params, hostname=hostname)
    if result.is_error():
        return result

    data = result.ok["data"]
    repo = data["repository"]
    parent = repo["parent"]
    upstream = (
        _parse_repository_from_dict(parent, hostname=hostname) if parent else None
    )
    repository = _parse_repository_from_dict(repo, hostname=hostname, upstream=upstream)
    return Result(ok=repository)


@dataclass
class PullRequestDetails:
    node_id: str
    number: int
    url: str
    head_oid: str
    head_branch_name: str


async def get_pull_request_details(
    pr: PullRequestId,
) -> Result[PullRequestDetails]:
    query = """
query ($owner: String!, $name: String!, $number: Int!) {
  repository(name: $name, owner: $owner) {
    pullRequest(number: $number) {
      id
      url
      headRefOid
      headRefName
    }
  }
}
"""
    params = {
        "query": query,
        "owner": pr.owner,
        "name": pr.name,
        "number": pr.number,
    }
    result = await make_request(params, hostname=pr.get_hostname())
    if result.is_error():
        return result

    data = result.ok["data"]["repository"]["pullRequest"]
    return Result(
        ok=PullRequestDetails(
            node_id=data["id"],
            number=pr.number,
            url=data["url"],
            head_oid=data["headRefOid"],
            head_branch_name=data["headRefName"],
        )
    )


def _parse_repository_from_dict(repo_obj, hostname: str, upstream=None) -> Repository:
    default_branch = "main" if "defaultBranchRef" not in repo_obj else repo_obj["defaultBranchRef"]["name"]
    return Repository(
        id=repo_obj["id"],
        hostname=hostname,
        owner=repo_obj["owner"]["login"],
        name=repo_obj["name"],
        default_branch=default_branch,
        is_fork=repo_obj["isFork"],
        upstream=upstream,
    )


async def guess_next_pull_request_number(
    hostname: str, owner: str, name: str
) -> Result[int]:
    """Returns our best guess as to the number that will be assigned to the next
    pull request for the specified repo. It is a "guess" because it is based
    on the largest number for either issues or pull requests seen thus far and
    adds 1 to it. This "guess" can be wrong if:

    - The most recent pull request/issue has been deleted, in which case the
      next number would be one more than that.
    - If an issue/pull request is created between the time this function is
      called and the pull request is created, the guess will also be wrong.

    Note that the only reason we bother to do this is because, at least at the
    time of this writing, we cannot rename  the branch used for the head of a
    pull request [programmatically] without closing the pull request.

    While there is an official GitHub API for renaming a branch, it closes all
    pull requests that have their `head` set to the old branch name!
    Unfortunately, this is not documented on:

    https://docs.github.com/en/rest/branches/branches#rename-a-branch

    Support for renaming a branch WITHOUT closing all of the pull requests was
    introduced in Jan 2021, but it only appears to be available via the Web UI:

    https://github.blog/changelog/2021-01-19-support-for-renaming-an-existing-branch/

    The endpoint the web UI hits is on github.com, not api.github.com, so it
    does not appear to be accessible to us.
    """
    query = """
query ($owner: String!, $name: String!) {
  repository(owner: $owner, name: $name) {
    issues(orderBy: {field: CREATED_AT, direction: ASC}, last: 1) {
      nodes {
        number
      }
    }
    pullRequests(orderBy: {field: CREATED_AT, direction: ASC}, last: 1) {
      nodes {
        number
      }
    }
  }
}
"""
    params: Dict[str, _Params] = {"query": query, "owner": owner, "name": name}
    result = await make_request(params, hostname=hostname)
    if result.is_error():
        return result

    # Find the max value of the fields, though note that it is possible no
    # issues or pull requests have ever been filed.
    repository = result.ok["data"]["repository"]

    def get_value(field):
        nodes = repository[field]["nodes"]
        return nodes[0]["number"] if nodes else 0

    values = [get_value(field) for field in ["issues", "pullRequests"]]
    next_number = max(*values) + 1
    return Result(ok=next_number)


async def create_pull_request(
    hostname: str, owner: str, name: str, base: str, head: str, title: str, body: str
) -> Result:
    endpoint = f"repos/{owner}/{name}/pulls"
    params: Dict[str, _Params] = {
        "base": base,
        "head": head,
        "title": title,
        "body": body,
    }
    return await make_request(params, hostname=hostname, endpoint=endpoint)


async def update_pull_request(
    hostname: str, node_id: str, title: str, body: str
) -> Result[str]:
    """Returns an "ID!" for the pull request, which should match the node_id
    that was passed in.
    """
    query = """
mutation ($pullRequestId: ID!, $title: String!, $body: String!) {
  updatePullRequest(
    input: {pullRequestId: $pullRequestId, title: $title, body: $body}
  ) {
    pullRequest {
      id
    }
  }
}
"""
    params: Dict[str, _Params] = {
        "query": query,
        "pullRequestId": node_id,
        "title": title,
        "body": body,
    }
    result = await make_request(params, hostname=hostname)
    if result.is_error():
        return result
    else:
        return Result(ok=result.ok["data"]["updatePullRequest"]["pullRequest"]["id"])


async def create_branch(
    *, hostname: str, repo_id: str, branch_name: str, oid: str
) -> Result[str]:
    """Attempts to create the branch. If successful, returns the ID of the newly
    created Ref.
    """
    query = """
mutation ($repositoryId: ID!, $name: String!, $oid: GitObjectID!) {
  createRef(input: {repositoryId: $repositoryId, name: $name, oid: $oid}) {
    ref {
      id
    }
  }
}
"""
    params: Dict[str, _Params] = {
        "query": query,
        "repositoryId": repo_id,
        "name": f"refs/heads/{branch_name}",
        "oid": oid,
    }
    result = await make_request(params, hostname=hostname)
    if result.is_error():
        return result
    else:
        return Result(ok=result.ok["data"]["createRef"]["ref"]["id"])


async def merge_into_branch(
    *, hostname: str, repo_id: str, oid_to_merge: str, branch_name: str
) -> Result[str]:
    """Takes the hash, oid_to_merge, and merges it into the specified branch_name."""
    query = """
mutation ($repositoryId: ID!, $base: String!, $head: String!) {
  mergeBranch(input: {repositoryId: $repositoryId, base: $base, head: $head}) {
    mergeCommit {
      oid
    }
  }
}
"""
    params: Dict[str, _Params] = {
        "query": query,
        "repositoryId": repo_id,
        "base": branch_name,
        "head": oid_to_merge,
    }
    result = await make_request(params, hostname=hostname)
    if result.is_error():
        return result
    else:
        return Result(ok=result.ok["data"]["mergeBranch"]["mergeCommit"]["oid"])


async def get_username(hostname: str) -> Result[str]:
    """Returns the username associated with the auth token. Note that it is
    slightly faster to call graphql.try_parse_oath_token_from_hosts_yml() and
    read the value from hosts.yml.
    """
    query = """
query {
  viewer {
    login
  }
}
"""
    params: Dict[str, _Params] = {
        "query": query,
    }
    result = await make_request(params, hostname=hostname)
    if result.is_error():
        return result
    else:
        return Result(ok=result.ok["data"]["viewer"]["login"])
