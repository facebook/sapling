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

from edenscm.i18n import _
from edenscm.result import Err, Ok, Result
from ghstack import github_gh_cli as gh_cli
from ghstack.github_gh_cli import JsonDict

from .consts import query
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


async def get_repository(
    hostname: str, owner: str, name: str
) -> Result[Repository, str]:
    """Returns an "ID!" for the repository that is necessary in other
    GitHub API calls.
    """
    params: Dict[str, _Params] = {
        "query": query.GRAPHQL_GET_REPOSITORY,
        "owner": owner,
        "name": name,
    }
    result = await gh_cli.make_request(params, hostname=hostname)
    if result.is_err():
        return Err(result.unwrap_err())

    data = result.unwrap()["data"]
    repo = data["repository"]
    parent = repo["parent"]

    if parent:
        result = _parse_repository_from_dict(parent, hostname=hostname)
        if result.is_err():
            return result
        else:
            upstream = result.unwrap()
    else:
        upstream = None
    return _parse_repository_from_dict(repo, hostname=hostname, upstream=upstream)


@dataclass
class PullRequestDetails:
    node_id: str
    number: int
    url: str
    base_oid: str
    base_branch_name: str
    head_oid: str
    head_branch_name: str
    # body should be the pull request body as authored by the user (i.e.,
    # containing Markdown source), as opposed to:
    #   bodyText: plaintext version of body with Markdown markup removed
    #   bodyHTML: body rendered as "safe" HTML
    body: str


async def get_pull_request_details(
    pr: PullRequestId,
) -> Result[PullRequestDetails, str]:
    params = {
        "query": query.GRAPHQL_GET_PULL_REQUEST,
        "owner": pr.owner,
        "name": pr.name,
        "number": pr.number,
    }
    result = await gh_cli.make_request(params, hostname=pr.get_hostname())
    if result.is_err():
        return Err(result.unwrap_err())

    data = result.unwrap()["data"]["repository"]["pullRequest"]
    return Ok(
        PullRequestDetails(
            node_id=data["id"],
            number=pr.number,
            url=data["url"],
            base_oid=data["baseRefOid"],
            base_branch_name=data["baseRefName"],
            head_oid=data["headRefOid"],
            head_branch_name=data["headRefName"],
            body=data["body"],
        )
    )


def _parse_repository_from_dict(
    repo_obj, hostname: str, upstream=None
) -> Result[Repository, str]:
    owner = repo_obj["owner"]["login"]
    name = repo_obj["name"]
    branch_ref = repo_obj["defaultBranchRef"]
    if branch_ref is None:
        error_message = (
            _(
                """\
This repository has no default branch. This is likely because it is empty.

Consider using %s to initialize your
repository.
"""
            )
            % f"https://{hostname}/{owner}/{name}/new/main"
        )
        return Err(error_message)
    return Ok(
        Repository(
            id=repo_obj["id"],
            hostname=hostname,
            owner=owner,
            name=name,
            default_branch=branch_ref["name"],
            is_fork=repo_obj["isFork"],
            upstream=upstream,
        )
    )


async def create_pull_request_placeholder_issue(
    hostname: str,
    owner: str,
    name: str,
) -> Result[int, str]:
    """creates a GitHub issue for the purpose of reserving an issue number"""
    endpoint = f"repos/{owner}/{name}/issues"
    params: Dict[str, _Params] = {
        "title": "placeholder for pull request",
    }
    result = await gh_cli.make_request(params, hostname=hostname, endpoint=endpoint)
    if result.is_err():
        return Err(result.unwrap_err())
    else:
        return Ok(result.unwrap()["number"])


async def create_pull_request(
    hostname: str,
    owner: str,
    name: str,
    base: str,
    head: str,
    body: str,
    issue: int,
    is_draft: bool = False,
) -> Result[JsonDict, str]:
    """Creates a new pull request by converting an existing issue into a PR.

    Note that `title` and `issue` are mutually exclusive fields when creating a
    pull request.

    Note that the documented HTTP response status codes
    (https://docs.github.com/en/rest/pulls/pulls?apiVersion=2022-11-28#create-a-pull-request--status-codes)
    for this REST endpoint are:

    201 Created
    403 Forbidden
    422 Validation failed, or the endpoint has been spammed.

    In the event of a failure, *ideally* we would close or delete the
    placeholder issue (or even better, save it for later use), but that seems
    tricky do here because:

    403 If creating a PR for the issue is forbidden, closing it probably is, too.
    422 If the endpoint has been spammed, then it seems unlikely that making
        *another* request to the endpoint to close the issue will succeed.

    TODO: Figure out some sort of error-recovery scheme. Note that
    make_request() returns an error as a string that may or may not be valid
    JSON, so we do not have a programmatic way to determine the type of error.
    """
    endpoint = f"repos/{owner}/{name}/pulls"
    params: Dict[str, _Params] = {
        "base": base,
        "head": head,
        "body": body,
        "issue": issue,
        "draft": is_draft,
    }
    return await gh_cli.make_request(params, hostname=hostname, endpoint=endpoint)


async def update_pull_request(
    hostname: str,
    node_id: str,
    title: str,
    body: str,
    base: str,
) -> Result[str, str]:
    """Returns an "ID!" for the pull request, which should match the node_id
    that was passed in.
    """
    params: Dict[str, _Params] = {
        "query": query.GRAPHQL_UPDATE_PULL_REQUEST,
        "pullRequestId": node_id,
        "title": title,
        "body": body,
        "base": base,
    }
    result = await gh_cli.make_request(params, hostname=hostname)
    if result.is_err():
        return Err(result.unwrap_err())
    else:
        return Ok(result.unwrap()["data"]["updatePullRequest"]["pullRequest"]["id"])


async def create_branch(
    *, hostname: str, repo_id: str, branch_name: str, oid: str
) -> Result[str, str]:
    """Attempts to create the branch. If successful, returns the ID of the newly
    created Ref.
    """
    params: Dict[str, _Params] = {
        "query": query.GRAPHQL_CREATE_BRANCH,
        "repositoryId": repo_id,
        "name": f"refs/heads/{branch_name}",
        "oid": oid,
    }
    result = await gh_cli.make_request(params, hostname=hostname)
    if result.is_err():
        return Err(result.unwrap_err())
    else:
        return Ok(result.unwrap()["data"]["createRef"]["ref"]["id"])


async def merge_into_branch(
    *, hostname: str, repo_id: str, oid_to_merge: str, branch_name: str
) -> Result[str, str]:
    """Takes the hash, oid_to_merge, and merges it into the specified branch_name.

    - base must be a branch name
    - oid_to_merge is the head to merge into base: can be a branch name or an oid
    """
    params: Dict[str, _Params] = {
        "query": query.GRAPHQL_MERGE_BRANCH,
        "repositoryId": repo_id,
        "base": branch_name,
        "head": oid_to_merge,
    }
    result = await gh_cli.make_request(params, hostname=hostname)
    if result.is_err():
        return Err(result.unwrap_err())
    else:
        return Ok(result.unwrap()["data"]["mergeBranch"]["mergeCommit"]["oid"])


async def get_username(hostname: str) -> Result[str, str]:
    """Returns the username associated with the auth token. Note that it is
    slightly faster to call graphql.try_parse_oath_token_from_hosts_yml() and
    read the value from hosts.yml.
    """
    params: Dict[str, _Params] = {
        "query": query.GRAPHQL_GET_LOGIN,
    }
    result = await gh_cli.make_request(params, hostname=hostname)
    if result.is_err():
        return Err(result.unwrap_err())
    else:
        return Ok(result.unwrap()["data"]["viewer"]["login"])
