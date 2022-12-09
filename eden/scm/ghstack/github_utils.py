import re
from typing import List, Optional, TypedDict

from edenscm import error
from edenscm.i18n import _
import ghstack.github
import ghstack.shell
from ghstack.ghs_types import GitCommitHash, GhNumber, GitHubRepositoryId, GitTreeHash

GitHubRepoNameWithOwner = TypedDict('GitHubRepoNameWithOwner', {
    'owner': str,
    'name': str,
})


def get_github_repo_name_with_owner(
    *,
    sh: ghstack.shell.Shell,
    github_url: str,
    remote_name: str,
) -> GitHubRepoNameWithOwner:
    # Grovel in remotes to figure it out
    remote_url = sh.git("remote", "get-url", remote_name)
    while True:
        match = r'^git@{github_url}:([^/]+)/(.+?)(?:\.git)?$'.format(
            github_url=github_url
        )
        m = re.match(match, remote_url)
        if m:
            owner = m.group(1)
            name = m.group(2)
            break
        search = r'{github_url}/([^/]+)/(.+?)(?:\.git)?$'.format(
            github_url=github_url
        )
        m = re.search(search, remote_url)
        if m:
            owner = m.group(1)
            name = m.group(2)
            break
        raise RuntimeError(
            "Couldn't determine repo owner and name from url: {}"
            .format(remote_url))
    return {'owner': owner, 'name': name}


GitHubRepoInfo = TypedDict('GitHubRepoInfo', {
    'name_with_owner': GitHubRepoNameWithOwner,
    'id': GitHubRepositoryId,
    'is_fork': bool,
    'default_branch': str,
})


def get_github_repo_info(
    *,
    github: ghstack.github.GitHubEndpoint,
    sh: ghstack.shell.Shell,
    repo_owner: Optional[str] = None,
    repo_name: Optional[str] = None,
    github_url: str,
    remote_name: str,
) -> GitHubRepoInfo:
    if repo_owner is None or repo_name is None:
        name_with_owner = get_github_repo_name_with_owner(
            sh=sh,
            github_url=github_url,
            remote_name=remote_name,
        )
    else:
        name_with_owner: GitHubRepoNameWithOwner = {"owner": repo_owner, "name": repo_name}

    owner = name_with_owner["owner"]
    name = name_with_owner["name"]

    # TODO: Cache this guy
    repo = github.graphql_sync(
        """
        query ($owner: String!, $name: String!) {
            repository(name: $name, owner: $owner) {
                id
                isFork
                defaultBranchRef {
                    name
                }
            }
        }""",
        owner=owner,
        name=name)["data"]["repository"]

    # Note for a new repo without any commits, this will be null in the GraphQL
    # response.
    branch_ref = repo["defaultBranchRef"]
    if branch_ref is None:
        raise error.Abort(_("""\
This repository has no default branch. This is likely because it is empty.

Consider using %s to initialize your
repository.
""") % f"https://{github_url}/{owner}/{name}/new/main")

    return {
        "name_with_owner": name_with_owner,
        "id": repo["id"],
        "is_fork": repo["isFork"],
        "default_branch": branch_ref["name"],
    }


RE_PR_URL = re.compile(
    r'^https://(?P<github_url>[^/]+)/(?P<owner>[^/]+)/(?P<name>[^/]+)/pull/(?P<number>[0-9]+)/?$')

GitHubPullRequestParams = TypedDict('GitHubPullRequestParams', {
    'github_url': str,
    'owner': str,
    'name': str,
    'number': int,
})


def parse_pull_request(pull_request: str) -> GitHubPullRequestParams:
    m = RE_PR_URL.match(pull_request)
    if not m:
        raise RuntimeError("Did not understand PR argument.  PR must be URL")

    github_url = m.group("github_url")
    owner = m.group("owner")
    name = m.group("name")
    number = int(m.group("number"))
    return {'github_url': github_url, 'owner': owner, 'name': name, 'number': number}


def lookup_pr_to_orig_ref(github: ghstack.github.GitHubEndpoint, *, owner: str, name: str, number: int) -> str:
    pr_result = github.graphql_sync("""
        query ($owner: String!, $name: String!, $number: Int!) {
            repository(name: $name, owner: $owner) {
                pullRequest(number: $number) {
                    headRefName
                }
            }
        }
    """, owner=owner, name=name, number=number)
    head_ref = pr_result["data"]["repository"]["pullRequest"]["headRefName"]
    assert isinstance(head_ref, str)
    orig_ref = re.sub(r'/head$', '/orig', head_ref)
    if orig_ref == head_ref:
        raise RuntimeError("The ref {} doesn't look like a ghstack reference".format(head_ref))
    return orig_ref


GitCommitAndTree = TypedDict('GitCommitAndTree', {
    'commit': GitCommitHash,
    'tree': GitTreeHash,
})


def get_commit_and_tree_for_ref(
    *,
    github: ghstack.github.GitHubEndpoint,
    repo_id: GitHubRepositoryId,
    ref: str,
) -> GitCommitAndTree:
    target = github.graphql_sync(
    """
      query ($repo_id: ID!, $ref: String!) {
        node(id: $repo_id) {
          ... on Repository {
            ref(qualifiedName: $ref) {
              target {
                oid
                ... on Commit {
                  tree {
                    oid
                  }
                }
              }
            }
          }
        }
      }
    """,
        repo_id=repo_id,
        ref=ref,
    )["data"]["node"]["ref"]["target"]
    commit = GitCommitHash(target["oid"])
    tree = GitTreeHash(target["tree"]["oid"])
    return {'commit': commit, 'tree': tree}


def get_next_ghnum(
    *,
    github: ghstack.github.GitHubEndpoint,
    repo_id: GitHubRepositoryId,
    username: str,
) -> GhNumber:
    """Determine the next available GhNumber.  We do this by
    iterating through known branches and keeping track
    of the max.  The next available GhNumber is the next number.
    This is technically subject to a race, but we assume
    end user is not running this script concurrently on
    multiple machines (you bad bad)
    """
    branches = []
    cursor = ""
    # Call get_base_heads() successively, specifying the cursor, as appropriate,
    # to ensure we get all of the heads.
    while True:
        base_heads = get_base_heads(
            github=github, repo_id=repo_id, username=username, after_cursor=cursor
        )
        branches.extend(base_heads["branches"])
        cursor = base_heads["cursor"]
        if cursor is None:
            break

    if branches:
        max_ref_num = max(int(b.split("/", 1)[0]) for b in branches)
        return GhNumber(str(max_ref_num + 1))
    else:
        return GhNumber("0")


BaseHeads = TypedDict(
    "BaseHeads",
    {
        # Values should be of the form: "1262/base".
        "branches": List[str],
        "cursor": Optional[str],
    },
)


def get_base_heads(
    *,
    github: ghstack.github.GitHubEndpoint,
    repo_id: GitHubRepositoryId,
    username: str,
    after_cursor: str = "",
) -> BaseHeads:
    ref_prefix = f"refs/heads/gh/{username}/"
    data = github.graphql_sync(
        """
      query ($repo_id: ID!, $ref_prefix: String!, $after: String!) {
        node(id: $repo_id) {
          ... on Repository {
            refs(
              refPrefix: $ref_prefix
              first: 100
              query: "/base"
              orderBy: {direction: DESC, field: ALPHABETICAL}
              after: $after
            ) {
              edges {
                node {
                  branchName: name
                }
              }
              pageInfo {
                endCursor
              }
            }
          }
        }
      }
    """,
        repo_id=repo_id,
        ref_prefix=ref_prefix,
        after=after_cursor,
    )["data"]
    refs = data["node"]["refs"]
    branches = [edge["node"]["branchName"] for edge in refs["edges"]]
    cursor = refs["pageInfo"]["endCursor"]
    return {
        "branches": branches,
        "cursor": cursor,
    }


def update_ref(
    *,
    github: ghstack.github.GitHubEndpoint,
    repo_id: GitHubRepositoryId,
    ref: str,
    target_ref: str,
) -> str:
    """Updates ref to point to the same commit that target_ref points to.
    """
    ref_id = get_id_for_ref(
        github=github,
        repo_id=repo_id,
        ref=ref,
    )

    target_oid = get_commit_and_tree_for_ref(
        github=github,
        repo_id=repo_id,
        ref=target_ref,
    )['commit']

    data = github.graphql_sync(
        """
        mutation ($refId: ID!, $oid: GitObjectID!) {
            updateRef(input: {
                refId: $refId,
                oid: $oid
            }) {
                ref {
                    id
                    name
                    target {
                        oid
                    }
                }
            }
        }
        """,
        refId=ref_id,
        oid=target_oid,
    )["data"]
    return data["updateRef"]["ref"]["target"]["oid"]


def get_id_for_ref(
    *,
    github: ghstack.github.GitHubEndpoint,
    repo_id: GitHubRepositoryId,
    ref: str,
) -> GitCommitAndTree:
    return github.graphql_sync(
    """
      query ($repo_id: ID!, $ref: String!) {
        node(id: $repo_id) {
          ... on Repository {
            ref(qualifiedName: $ref) {
              id
            }
          }
        }
      }
    """,
        repo_id=repo_id,
        ref=ref,
    )["data"]["node"]["ref"]["id"]
