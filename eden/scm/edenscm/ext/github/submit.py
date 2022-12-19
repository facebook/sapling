# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import asyncio
import re
import subprocess
from dataclasses import dataclass
from enum import Enum
from typing import Any, Callable, List, Optional, Tuple, TypeVar

from edenscm import error, git, gituser, gpg
from edenscm.i18n import _
from edenscm.node import hex, nullid
from ghstack.github_gh_cli import Result

from . import gh_submit, github_repo_util
from .gh_submit import PullRequestDetails, Repository
from .github_repo_util import check_github_repo, GitHubRepo
from .pr_parser import get_pull_request_for_context

from .pullrequest import PullRequestId
from .pullrequeststore import PullRequestStore


def submit(ui, repo, *args, **opts):
    """Create or update GitHub pull requests."""
    github_repo = check_github_repo(repo)
    is_draft = opts.get("draft")
    return asyncio.run(
        update_commits_in_stack(ui, repo, github_repo, is_draft=is_draft)
    )


class SubmitWorkflow(Enum):
    """Various workflows supported by `sl pr submit`."""

    """The "classic" GitHub pull request workflow where `main` is used as the
    base branch for the PR and the PR contains an arbitrary number of commits
    between the base and the head branch. For a Git user, the feature branch and
    named branch are generally one in the same, so it is used as the head branch
    for the PR. In Sapling, the commit that is used as the head of the pull
    request is somewhere on the feature branch, but it is not necessarily the
    tip.
    """
    CLASSIC = "classic"

    """A "stacked diff" workflow where each pull request contains a single
    commit. Creates synthetic base and head branches for each PR to ensure each
    continues to include exactly one commit. Updates to PRs often require
    force pushing. (An alternative that avoids force-pushing is `sl ghstack`.)
    """
    SINGLE = "single"

    """An "overlapping" workflow where all PRs contain a common base branch
    (`main`) while each commit in the stack gets its own pull request using
    itself as the head branch. This approach is confusing to review in GitHub's
    pull request UI (ReviewStack is strongly recommended in this case), but it
    is supported to accommodate users who do not have write access to the GitHub
    repo.
    """
    OVERLAP = "overlap"

    @staticmethod
    def from_config(ui) -> "SubmitWorkflow":
        workflow = ui.config("github", "pr_workflow")
        if not workflow or workflow == "overlap":
            # For now, default to OVERLAP.
            return SubmitWorkflow.OVERLAP
        elif workflow == "single":
            return SubmitWorkflow.SINGLE
        else:
            # Note that "classic" is not recognized yet.
            ui.warn(
                _("unrecognized config for github.pr_workflow: defaulting to 'overlap'")
            )
            return SubmitWorkflow.OVERLAP


@dataclass
class CommitData:
    """The data we need about each commit to support `submit`."""

    # Commit ID.
    node: bytes
    # If present, should match pr.head_branch_name.
    head_branch_name: Optional[str]
    # If present, the existing pull request this commit (and possibly some of
    # its immediate ancestors) is associated with.
    pr: Optional[PullRequestDetails]
    # The context for the node.
    ctx: Any
    # Whether this commit should be part of another pull request rather than
    # the head of its own pull request.
    is_dep: bool
    # Description of the commit, set lazily.
    msg: Optional[str] = None

    def get_msg(self) -> str:
        if self.msg is None:
            self.msg = self.ctx.description()
        return self.msg


@dataclass
class CommitNeedsPullRequest:
    commit: CommitData
    parent: Optional[CommitData]


@dataclass
class PullRequestParams:
    """Information necessary to create a pull request for a single commit."""

    commit: CommitData
    parent: Optional[CommitData]
    number: int


async def update_commits_in_stack(
    ui, repo, github_repo: GitHubRepo, is_draft: bool
) -> int:
    parents = repo.dirstate.parents()
    if parents[0] == nullid:
        ui.status_err(_("commit has no parent: currently unsupported\n"))
        return 1

    store = PullRequestStore(repo)
    commits_to_process = await asyncio.gather(
        *[
            derive_commit_data(node, repo, store)
            for node in repo.nodes("sort(. %% public(), -rev)")
        ]
    )

    if not commits_to_process:
        ui.status_err(_("no commits to submit\n"))
        return 0

    workflow = SubmitWorkflow.from_config(ui)

    # Partition the chain.
    partitions: List[List[CommitData]] = []
    for commit in commits_to_process:
        if commit.is_dep:
            if partitions:
                partitions[-1].append(commit)
            else:
                # If the top of the stack is a "dep commit", then do not
                # submit it.
                continue
        else:
            partitions.append([commit])

    if not partitions:
        # It is possible that all of the commits_to_process were marked as
        # followers.
        ui.status_err(_("no commits to submit\n"))
        return 0

    origin = get_origin(ui)

    # git push --force any heads that need updating, creating new branch names,
    # if necessary.
    refs_to_update: List[str] = []
    commits_that_need_pull_requests: List[CommitNeedsPullRequest] = []

    # Note that `partitions` is ordered from the top of the stack to the bottom,
    # but we want to create PRs from the bottom to the top so the PR numbers are
    # created in ascending order.
    parent_commit = None
    for partition in reversed(partitions):
        commit = partition[0]
        pr = commit.pr
        if pr:
            if pr.head_oid == hex(commit.node):
                ui.status_err(_("#%d is up-to-date\n") % pr.number)
            else:
                refs_to_update.append(
                    f"{hex(commit.node)}:refs/heads/{pr.head_branch_name}"
                )
        else:
            commit_needs_pr = CommitNeedsPullRequest(
                commit=commit, parent=parent_commit
            )
            commits_that_need_pull_requests.append(commit_needs_pr)
        parent_commit = commit

    # Reserve one GitHub issue number for each pull request (in parallel) and
    # then assign them in increasing order. Also ensure head_branch_name is set
    # on every CommitData.
    repository: Optional[Repository] = None
    pull_requests_to_create: List[PullRequestParams] = []
    if commits_that_need_pull_requests:
        repository = await get_repository_for_origin(origin, github_repo.hostname)
        issue_numbers = await _create_placeholder_issues(
            repository, len(commits_that_need_pull_requests)
        )
        for commit_needs_pr, number in zip(
            commits_that_need_pull_requests, issue_numbers
        ):
            # Consider including username in branch_name?
            branch_name = f"pr{number}"
            commit = commit_needs_pr.commit
            commit.head_branch_name = branch_name
            refs_to_update.append(f"{hex(commit.node)}:refs/heads/{branch_name}")
            params = PullRequestParams(
                commit=commit, parent=commit_needs_pr.parent, number=number
            )
            pull_requests_to_create.append(params)

    gitdir = None

    def get_gitdir() -> str:
        nonlocal gitdir
        if gitdir is None:
            gitdir = git.readgitdir(repo)
            if not gitdir:
                raise error.Abort(_("could not find gitdir"))
        return gitdir

    if refs_to_update:
        gitdir = get_gitdir()
        git_push_args = ["push", "--force", origin] + refs_to_update
        ui.status_err(_("pushing %d to %s\n") % (len(refs_to_update), origin))
        run_git_command(git_push_args, gitdir)
    else:
        ui.status_err(_("no pull requests to update\n"))
        return 0

    if pull_requests_to_create:
        assert repository is not None
        await create_pull_requests(
            pull_requests_to_create, workflow, repository, store, ui, is_draft
        )

    # Now that each pull request has a named branch pushed to GitHub, we can
    # create/update the pull request title and body, as appropriate.
    pr_numbers_and_num_commits = [
        (none_throws(p[0].pr).number, len(p)) for p in partitions
    ]

    # Add the head of the stack to the sapling-pr-archive branch.
    tip = hex(partitions[0][0].node)

    if not repository:
        repository = await get_repository_for_origin(origin, github_repo.hostname)
    rewrite_and_archive_requests = [
        rewrite_pull_request_body(
            partitions, index, pr_numbers_and_num_commits, repository, ui
        )
        for index in range(len(partitions))
    ] + [
        add_pr_head_to_archives(
            ui=ui, origin=origin, repository=repository, tip=tip, get_gitdir=get_gitdir
        )
    ]
    await asyncio.gather(*rewrite_and_archive_requests)
    return 0


async def rewrite_pull_request_body(
    partitions: List[List[CommitData]],
    index: int,
    pr_numbers_and_num_commits: List[Tuple[int, int]],
    repository: Repository,
    ui,
):
    # If available, use the head branch of the previous partition as the base
    # of this branch. Recall that partitions is ordered from the top of the
    # stack to the bottom.
    partition = partitions[index]
    if index == len(partitions) - 1:
        base = repository.get_base_branch()
    else:
        base = none_throws(partitions[index + 1][0].head_branch_name)

    head_commit_data = partition[0]
    commit_msg = head_commit_data.get_msg()
    title, body = create_pull_request_title_and_body(
        commit_msg,
        pr_numbers_and_num_commits,
        index,
        repository,
    )
    pr = head_commit_data.pr
    assert pr
    result = await gh_submit.update_pull_request(
        repository.hostname, pr.node_id, title, body, base
    )
    if result.is_error():
        ui.status_err(
            _("warning, updating #%d may not have succeeded: %s\n")
            % (pr.number, result.error)
        )
    else:
        ui.status_err(_("updated body for %s\n") % pr.url)


async def create_pull_requests(
    commits: List[PullRequestParams],
    workflow: SubmitWorkflow,
    repository: Repository,
    store: PullRequestStore,
    ui,
    is_draft: bool,
) -> None:
    """Creates a new pull request for each entry in the `commits` list.

    Each entry in `commits` is a (CommitData, branch_name, issue_number). Each
    CommitData will be updated such that its `.pr` field is set appropriately.
    """
    head_ref_prefix = f"{repository.owner}:" if repository.is_fork else ""
    owner, name = repository.get_upstream_owner_and_name()
    base_branch_for_repo = repository.get_base_branch()
    hostname = repository.hostname

    async def create_pull_request(params: PullRequestParams):
        commit = params.commit
        body = commit.get_msg()
        issue_number = params.number

        # Note that "overlapping" pull requests will all share the same base.
        base = base_branch_for_repo
        if workflow == SubmitWorkflow.SINGLE:
            parent = params.parent
            if parent:
                base = none_throws(parent.head_branch_name)

        response = await gh_submit.create_pull_request(
            hostname=repository.hostname,
            owner=owner,
            name=name,
            base=base,
            head=f"{head_ref_prefix}{none_throws(commit.head_branch_name)}",
            body=body,
            issue=issue_number,
            is_draft=is_draft,
        )
        # At this point, the title of the PR will be the placeholder value from
        # create_pull_request_placeholder_issue(), but so the caller is
        # responsible for ensuring update_pull_request() is eventually called.

        if response.is_error():
            raise error.Abort(
                _("error creating pull request for %s: %s")
                % (hex(commit.node), response.error)
            )

        # Because create_pull_request() uses the REST API instead of the
        # GraphQL API [where we would have to enumerate the fields we
        # want in the response], the response JSON appears to contain
        # "anything" we might want, but we only care about the number and URL.
        data = response.ok
        url = data["html_url"]
        ui.status_err(_("created new pull request: %s\n") % url)
        number = data["number"]
        if issue_number != number:
            ui.status_err(
                _("Issue number mismatch: supplied %d and received %d.\n")
                % (issue_number, number)
            )
        pr_id = PullRequestId(hostname=hostname, owner=owner, name=name, number=number)
        store.map_commit_to_pull_request(commit.node, pr_id)

        # Now that the pull request has been created, update the .pr field on
        # CommitData.
        pr = await get_pull_request_details_or_throw(pr_id)
        commit.pr = pr

    # Because the issue numbers have been reserved in advance, each pull request
    # can be created in parallel.
    await asyncio.gather(*[create_pull_request(c) for c in commits])


def create_pull_request_title_and_body(
    commit_msg: str,
    pr_numbers_and_num_commits: List[Tuple[int, int]],
    pr_numbers_index: int,
    repository: Repository,
) -> Tuple[str, str]:
    r"""Returns (title, body) for the pull request.

    >>> commit_msg = 'The original commit message.\nSecond line of message.'
    >>> pr_numbers_and_num_commits = [(1, 1), (2, 2), (42, 1), (4, 1)]
    >>> pr_numbers_index = 2
    >>> upstream_repo = Repository(hostname="github.com", id="abcd=", owner="facebook", name="sapling", default_branch="main", is_fork=False)
    >>> contributor_repo = Repository(hostname="github.com", id="efgh=", owner="keith", name="sapling", default_branch="main", is_fork=True, upstream=upstream_repo)
    >>> title, body = create_pull_request_title_and_body(
    ...     commit_msg,
    ...     pr_numbers_and_num_commits,
    ...     pr_numbers_index,
    ...     contributor_repo,
    ... )
    >>> title == 'The original commit message.'
    True
    >>> reviewstack_url = "https://reviewstack.dev/facebook/sapling/pull/42"
    >>> body == ('Stack created with [Sapling](https://sapling-scm.com). ' +
    ...     f'Best reviewed with [ReviewStack]({reviewstack_url}).\n' +
    ...     '* #1\n' +
    ...     '* #2 (2 commits)\n' +
    ...     '* __->__ #42\n' +
    ...     '* #4\n' +
    ...     '\n' +
    ...     'The original commit message.\n' +
    ...     'Second line of message.\n')
    True
    """
    owner, name = repository.get_upstream_owner_and_name()
    pr = pr_numbers_and_num_commits[pr_numbers_index][0]
    reviewstack_url = f"https://reviewstack.dev/{owner}/{name}/pull/{pr}"
    bulleted_list = "\n".join(
        [
            format_stack_entry(pr_number, index, pr_numbers_index, num_commits)
            for index, (pr_number, num_commits) in enumerate(pr_numbers_and_num_commits)
        ]
    )
    title = firstline(commit_msg)
    body = f"""Stack created with [Sapling](https://sapling-scm.com). Best reviewed with [ReviewStack]({reviewstack_url}).
{bulleted_list}

{commit_msg}
"""
    return title, body


def format_stack_entry(
    pr_number: int,
    pr_number_index: int,
    current_pr_index: int,
    num_commits: int,
) -> str:
    line = (
        f"* #{pr_number}"
        if current_pr_index != pr_number_index
        else f"* __->__ #{pr_number}"
    )
    if num_commits > 1:
        line += f" ({num_commits} commits)"
    return line


async def _create_placeholder_issues(repository: Repository, num: int) -> List[int]:
    """create the specified number of placeholder issues in parallel"""
    upstream_owner, upstream_name = repository.get_upstream_owner_and_name()
    issue_number_results = await asyncio.gather(
        *[
            gh_submit.create_pull_request_placeholder_issue(
                hostname=repository.hostname,
                owner=upstream_owner,
                name=upstream_name,
            )
            for _ in range(num)
        ]
    )

    def unwrap(r: Result[int]) -> int:
        if r.is_error():
            raise error.Abort(
                _(
                    "Error while trying to create a placeholder issue for a pull request on %s/%s: %s"
                )
                % (upstream_owner, upstream_name, r.error)
            )
        else:
            return none_throws(r.ok)

    issue_numbers = [unwrap(r) for r in issue_number_results]
    issue_numbers.sort()
    return issue_numbers


async def get_repository_for_origin(origin: str, hostname: str) -> Repository:
    origin_owner, origin_name = get_owner_and_name(origin)
    return await get_repo(hostname, origin_owner, origin_name)


def get_origin(ui) -> str:
    origin = ui.config("paths", "default")
    if origin:
        return origin
    else:
        raise error.Abort(_("paths.default not set in config"))


def get_owner_and_name(origin: str) -> Tuple[str, str]:
    github_repo = github_repo_util.parse_github_repo_from_github_url(origin)
    if github_repo:
        return (github_repo.owner, github_repo.name)
    else:
        raise error.Abort(_("could not parse GitHub owner and name from %s") % origin)


async def get_repo(hostname: str, owner: str, name: str) -> Repository:
    repo_result = await gh_submit.get_repository(hostname, owner, name)
    repository = repo_result.ok
    if repository:
        return repository
    else:
        raise error.Abort(_("failed to fetch repo id: %s") % repo_result.error)


async def derive_commit_data(node: bytes, repo, store: PullRequestStore) -> CommitData:
    ctx = repo[node]
    pr_id = get_pull_request_for_context(store, ctx)
    pr = await get_pull_request_details_or_throw(pr_id) if pr_id else None
    msg = None
    if pr:
        is_dep = False
        head_branch_name = pr.head_branch_name
    else:
        msg = ctx.description()
        is_dep = store.is_follower(node)
        head_branch_name = None
    return CommitData(
        node=node,
        head_branch_name=head_branch_name,
        pr=pr,
        ctx=ctx,
        is_dep=is_dep,
        msg=msg,
    )


async def get_pull_request_details_or_throw(pr_id: PullRequestId) -> PullRequestDetails:
    result = await gh_submit.get_pull_request_details(pr_id)
    if result.is_error():
        raise error.Abort(_("error fetching %s: %s") % (pr_id.as_url(), result.error))
    else:
        return none_throws(result.ok)


async def add_pr_head_to_archives(
    *,
    ui,
    origin: str,
    repository: Repository,
    tip: str,
    get_gitdir: Callable[[], str],
):
    """Takes the specified commit (tip) and merges it into the appropriate
    archive branch for the (repo, username). GitHub will periodically garbage
    collect commits that are no longer part of a public branch, but we want to
    prevent this to ensure previous version of a PR can be viewed later, even
    after it has been updated via a force-push.

    tip is the hex version of the commit hash to be merged into the archive branch.
    """
    username = await get_username(hostname=repository.hostname)
    if not username:
        raise error.Abort(_("could not determine GitHub username"))

    branch_name = f"sapling-pr-archive-{username}"
    # Try to merge the tip directly, though this may fail if tip has already
    # been merged or if the branch has not been created before. We try to merge
    # without checking for the existence of the branch to try to avoid a TOCTOU
    # error.
    result = await gh_submit.merge_into_branch(
        hostname=repository.hostname,
        repo_id=repository.id,
        oid_to_merge=tip,
        branch_name=branch_name,
    )
    if not result.is_error():
        return

    import json

    # TODO: Store Result.error as Dict so we don't have to parse it again.
    err = none_throws(result.error)
    response = None
    try:
        response = json.loads(err)
    except json.JSONDecodeError:
        # response is not guaranteed to be valid JSON.
        pass

    if response and is_already_merged_error(response):
        # Nothing to do!
        return
    elif response and is_branch_does_not_exist_error(response):
        # Archive branch does not exist yet, so initialize it with the current
        # tip.
        result = await gh_submit.create_branch(
            hostname=repository.hostname,
            repo_id=repository.id,
            branch_name=branch_name,
            oid=tip,
        )
        if result.is_error():
            raise error.Abort(
                _("unexpected error when trying to create branch %s with commit %s: %s")
                % (branch_name, tip, result.error)
            )
    elif response and is_merge_conflict(response):
        # Git cannot do the merge on its own, so we need to generate our own
        # commit that merges the existing archive with the contents of `tip` to
        # use as the new head for the archive branch.
        gitdir = get_gitdir()

        # We must fetch the archive branch because we need to have the commit
        # object locally in order to use it with commit-tree.
        run_git_command(["fetch", origin, branch_name], gitdir)
        # `git fetch --verbose` does not appear to include the hash, so we must
        # use `git ls-remote` to get it.
        ls_remote_args = ["ls-remote", origin, branch_name]
        ls_remote_output = (
            run_git_command(ls_remote_args, gitdir=gitdir).decode().rstrip()
        )
        # oid and ref name should be separated by a tab character, but we use
        # '\s+' just to be safe.
        match = re.match(r"^([0-9a-f]+)\s+.*$", ls_remote_output)
        if not match:
            raise error.Abort(
                _("unexpected output from `%s`: %s")
                % (" ".join(ls_remote_args), ls_remote_output)
            )

        branch_name_oid = match[1]

        # This will be the tree to use for the merge commit. We could use the
        # tree for either `tip` or `branch_name_oid`, but since `tip` appears to
        # be "newer," we prefer it as it seems less likely to cause a merge
        # conflict the next time we update the archive branch.
        tree_oid = (
            run_git_command(["log", "--max-count=1", "--format=%T", tip], gitdir=gitdir)
            .decode()
            .rstrip()
        )

        # Synthetically create a new commit that has `tip` and the old branch
        # head as parents and force-push it as the new branch head.
        user_name, user_email = gituser.get_identity_or_raise(ui)
        keyid = gpg.get_gpg_keyid(ui)
        gpg_args = [f"-S{keyid}"] if keyid else []
        commit_tree_args = (
            [
                "-c",
                f"user.name={user_name}",
                "-c",
                f"user.email={user_email}",
                "commit-tree",
            ]
            + gpg_args
            + [
                "-m",
                "merge commit for archive created by Sapling",
                "-p",
                tip,
                "-p",
                branch_name_oid,
                tree_oid,
            ]
        )
        merge_commit = (
            run_git_command(
                commit_tree_args,
                gitdir,
            )
            .decode()
            .rstrip()
        )
        refspec = f"{merge_commit}:refs/heads/{branch_name}"
        git_push_args = [
            "push",
            "--force",
            origin,
            refspec,
        ]
        ui.status_err(_("force-pushing %s to %s\n") % (refspec, origin))
        run_git_command(git_push_args, gitdir)
    else:
        raise error.Abort(
            _("unexpected error when trying to merge %s into %s: %s")
            % (tip, branch_name, err)
        )


def is_already_merged_error(response) -> bool:
    r"""
    >>> response = {
    ...   "data": {
    ...     "mergeBranch": None
    ...   },
    ...   "errors": [
    ...     {
    ...       "type": "UNPROCESSABLE",
    ...       "path": [
    ...         "mergeBranch"
    ...       ],
    ...       "locations": [
    ...         {
    ...           "line": 2,
    ...           "column": 3
    ...         }
    ...       ],
    ...       "message": "Failed to merge: \"Already merged\""
    ...     }
    ...   ]
    ... }
    >>> is_already_merged_error(response)
    True
    """
    errors = response.get("errors")
    if not errors or not isinstance(errors, list):
        return False
    for err in errors:
        if err.get("type") != "UNPROCESSABLE":
            continue
        message = err.get("message")
        if isinstance(message, str) and "Already merged" in message:
            return True
    return False


def is_merge_conflict(response) -> bool:
    r"""
    >>> response = {
    ...   "data": {
    ...     "mergeBranch": None
    ...   },
    ...   "errors": [
    ...     {
    ...       "type": "UNPROCESSABLE",
    ...       "path": [
    ...         "mergeBranch"
    ...       ],
    ...       "locations": [
    ...         {
    ...           "line": 3,
    ...           "column": 3
    ...         }
    ...       ],
    ...       "message": "Failed to merge: \"Merge conflict\""
    ...     }
    ...   ]
    ... }
    >>> is_merge_conflict(response)
    True
    """
    errors = response.get("errors")
    if not errors or not isinstance(errors, list):
        return False
    for err in errors:
        if err.get("type") != "UNPROCESSABLE":
            continue
        message = err.get("message")
        if isinstance(message, str) and "Merge conflict" in message:
            return True
    return False


def is_branch_does_not_exist_error(response) -> bool:
    r"""
    >>> response = {
    ...   "data": {
    ...     "mergeBranch": None
    ...   },
    ...   "errors": [
    ...     {
    ...       "type": "NOT_FOUND",
    ...       "path": [
    ...         "mergeBranch"
    ...       ],
    ...       "locations": [
    ...         {
    ...           "line": 2,
    ...           "column": 3
    ...         }
    ...       ],
    ...       "message": "No such base."
    ...     }
    ...   ]
    ... }
    """
    errors = response.get("errors")
    if not errors or not isinstance(errors, list):
        return False
    for err in errors:
        if err.get("type") != "NOT_FOUND":
            continue
        message = err.get("message")
        if isinstance(message, str) and "No such base." in message:
            return True
    return False


async def get_username(hostname: str) -> Optional[str]:
    """Returns the username for the user authenticated with the GitHub CLI."""
    result = await gh_submit.get_username(hostname=hostname)
    if result.is_error():
        return None
    else:
        return none_throws(result.ok)


def run_git_command(args: List[str], gitdir: str) -> bytes:
    """Returns stdout as a bytes if the command is successful."""
    full_args = ["git", "--git-dir", gitdir] + args
    proc = subprocess.run(full_args, capture_output=True)
    if proc.returncode == 0:
        return proc.stdout
    else:
        raise error.Abort(
            _("`%s` failed with exit code %d: %s")
            % (
                " ".join(full_args),
                proc.returncode,
                f"stdout: {proc.stdout.decode()}\nstderr: {proc.stderr.decode()}\n",
            )
        )


EOL_PATTERN = re.compile(r"\r?\n")
MAX_FIRSTLINE_LEN = 120


def firstline(msg: str) -> str:
    r"""Returns the "first line" of a commit message to use for the title of a
    pull request.

    >>> firstline("foobar")
    'foobar'
    >>> firstline("foo\nbar")
    'foo'
    >>> firstline("foo\r\nbar")
    'foo'
    >>> firstline("x" * (MAX_FIRSTLINE_LEN + 1)) == "x" * MAX_FIRSTLINE_LEN
    True
    """
    match = EOL_PATTERN.search(msg)
    end = match.start() if match else len(msg)
    end = min(end, MAX_FIRSTLINE_LEN)
    return msg[:end]


_T = TypeVar("_T")


def none_throws(optional: Optional[_T], msg: str = "Unexpected None") -> _T:
    assert optional is not None, msg
    return optional
