# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import asyncio
import os
from dataclasses import dataclass
from enum import Enum
from typing import Any, List, Optional, Tuple

from sapling import error, git, hintutil
from sapling.i18n import _
from sapling.node import hex, nullid
from sapling.result import Result

from . import gh_submit, github_repo_util
from .archive_commit import add_commit_to_archives
from .gh_submit import PullRequestDetails, PullRequestState, Repository
from .github_repo_util import check_github_repo, GitHubRepo
from .none_throws import none_throws
from .pr_parser import get_pull_request_for_context
from .pull_request_body import create_pull_request_title_and_body, firstline
from .pullrequest import PullRequestId
from .pullrequeststore import PullRequestStore
from .run_git_command import run_git_command


def submit(ui, repo, *args, **opts) -> int:
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
        workflow = ui.config(
            "github", "pr-workflow", ui.config("github", "pr_workflow")
        )
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


async def get_partitions(ui, repo, store, filter) -> List[List[CommitData]]:
    commits_to_process = await asyncio.gather(
        *[derive_commit_data(node, repo, store) for node in repo.nodes(filter)]
    )
    if not commits_to_process:
        return []

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
    return partitions


async def update_commits_in_stack(
    ui, repo, github_repo: GitHubRepo, is_draft: bool
) -> int:
    parents = repo.dirstate.parents()
    if parents[0] == nullid:
        ui.status_err(_("commit has no parent: currently unsupported\n"))
        return 1

    store = PullRequestStore(repo)

    workflow = SubmitWorkflow.from_config(ui)

    partitions = await get_partitions(ui, repo, store, "sort(. %% public(), -rev)")
    if not partitions:
        ui.status_err(_("no commits to submit\n"))
        return 0
    origin = get_push_origin(ui)
    use_placeholder_strategy = ui.configbool("github", "placeholder-strategy")
    if use_placeholder_strategy:
        params = await create_placeholder_strategy_params(
            ui, partitions, github_repo, origin
        )
    else:
        params = await create_serial_strategy_params(
            ui, partitions, github_repo, origin
        )
    refs_to_update = params.refs_to_update

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

    repository = params.repository
    if params.pull_requests_to_create:
        assert repository is not None
        if use_placeholder_strategy:
            assert isinstance(params, PlaceholderStrategyParams)
            await create_pull_requests_from_placeholder_issues(
                params.pull_requests_to_create,
                workflow,
                repository,
                store,
                ui,
                is_draft,
            )
        else:
            assert isinstance(params, SerialStrategyParams)
            await create_pull_requests_serially(
                params.pull_requests_to_create,
                workflow,
                repository,
                store,
                ui,
                is_draft,
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
            partitions, index, workflow, pr_numbers_and_num_commits, repository, ui
        )
        for index in range(len(partitions))
    ] + [
        add_commit_to_archives(
            oid_to_archive=tip,
            ui=ui,
            origin=origin,
            repository=repository,
            get_gitdir=get_gitdir,
        )
    ]
    await asyncio.gather(*rewrite_and_archive_requests)
    return 0


async def rewrite_pull_request_body(
    partitions: List[List[CommitData]],
    index: int,
    workflow: SubmitWorkflow,
    pr_numbers_and_num_commits: List[Tuple[int, int]],
    repository: Repository,
    ui,
) -> None:
    # If available, use the head branch of the previous partition as the base
    # of this branch. Recall that partitions is ordered from the top of the
    # stack to the bottom.
    partition = partitions[index]
    base = repository.get_base_branch()
    if workflow == SubmitWorkflow.SINGLE and index < len(partitions) - 1:
        base = none_throws(partitions[index + 1][0].head_branch_name)

    head_commit_data = partition[0]

    pr = head_commit_data.pr
    assert pr

    title = None
    if ui.configbool("github", "preserve-pull-request-description"):
        commit_msg = pr.body
        title = pr.title
    else:
        commit_msg = head_commit_data.get_msg()

    title, body = create_pull_request_title_and_body(
        commit_msg,
        pr_numbers_and_num_commits,
        index,
        repository,
        title,
        reviewstack=ui.configbool("github", "pull-request-include-reviewstack"),
    )

    if pr.state != PullRequestState.OPEN:
        ui.status_err(
            _("warning, not updating #%d because it isn't open\n") % pr.number
        )
        hintutil.triggershow(ui, "unlink-closed-pr")
        return

    result = await gh_submit.update_pull_request(
        repository.hostname, pr.node_id, title, body, base
    )
    if result.is_err():
        ui.status_err(
            _("warning, updating #%d may not have succeeded: %s\n")
            % (pr.number, result.unwrap_err())
        )
    else:
        ui.status_err(_("updated body for %s\n") % pr.url)


@dataclass
class SerialStrategyParams:
    # git push --force any heads that need updating, creating new branch names,
    # if necessary.
    refs_to_update: List[str]
    # The str in the Tuple is the head branch name for the commit.
    pull_requests_to_create: List[Tuple[CommitData, str]]
    repository: Optional[Repository]


async def create_serial_strategy_params(
    ui,
    partitions: List[List[CommitData]],
    github_repo: GitHubRepo,
    origin: str,
) -> SerialStrategyParams:
    # git push --force any heads that need updating, creating new branch names,
    # if necessary.
    refs_to_update = []
    pull_requests_to_create: List[Tuple[CommitData, str]] = []

    # These are set lazily because they require GraphQL calls.
    next_pull_request_number = None
    repository: Optional[Repository] = None

    # Note that `partitions` is ordered from the top of the stack to the bottom,
    # but we want to create PRs from the bottom to the top so the PR numbers are
    # created in ascending order.
    for partition in reversed(partitions):
        top = partition[0]
        pr = top.pr
        if pr:
            if pr.head_oid == hex(top.node):
                ui.status_err(_("#%d is up-to-date\n") % pr.number)
            else:
                refs_to_update.append(
                    f"{hex(top.node)}:refs/heads/{pr.head_branch_name}"
                )
        else:
            # top.node will become the head of a new PR, so it needs a branch
            # name.
            if next_pull_request_number is None:
                repository = await get_repository_for_origin(
                    origin, github_repo.hostname
                )
                upstream_owner, upstream_name = repository.get_upstream_owner_and_name()
                result = await gh_submit.guess_next_pull_request_number(
                    github_repo.hostname, upstream_owner, upstream_name
                )
                if result.is_err():
                    raise error.Abort(
                        _(
                            "could not determine the next pull request number for %s/%s: %s"
                        )
                        % (upstream_owner, upstream_name, result.unwrap_err())
                    )
                else:
                    next_pull_request_number = result.unwrap()
            else:
                next_pull_request_number += 1
            # Consider including username in branch_name?
            branch_name = f"pr{next_pull_request_number}"
            refs_to_update.append(f"{hex(top.node)}:refs/heads/{branch_name}")
            top.head_branch_name = branch_name
            pull_requests_to_create.append((top, branch_name))

    return SerialStrategyParams(refs_to_update, pull_requests_to_create, repository)


async def create_pull_requests_serially(
    commits: List[Tuple[CommitData, str]],
    workflow: SubmitWorkflow,
    repository: Repository,
    store: PullRequestStore,
    ui,
    is_draft: bool,
) -> None:
    """Creates a new pull request for each entry in the `commits` list.

    Each CommitData in `commits` will be updated such that its `.pr` field is
    set appropriately.
    """
    head_ref_prefix = f"{repository.owner}:" if repository.is_fork else ""
    owner, name = repository.get_upstream_owner_and_name()
    hostname = repository.hostname

    # Create the pull requests in order serially to give us the best chance of
    # the number in the branch name matching that of the actual pull request.
    commits_to_update = []
    parent = None
    for commit, branch_name in commits:
        base = repository.get_base_branch()
        if workflow == SubmitWorkflow.SINGLE and parent:
            base = none_throws(parent.head_branch_name)

        body = commit.get_msg()
        title = firstline(body)
        result = await gh_submit.create_pull_request(
            hostname=repository.hostname,
            owner=owner,
            name=name,
            base=base,
            head=f"{head_ref_prefix}{branch_name}",
            title=title,
            body=body,
            is_draft=is_draft,
        )

        if result.is_err():
            raise error.Abort(
                _("error creating pull request for %s: %s")
                % (hex(commit.node), result.unwrap_err())
            )

        # Because create_pull_request() uses the REST API instead of the
        # GraphQL API [where we would have to enumerate the fields we
        # want in the response], the response JSON appears to contain
        # "anything" we might want, but we only care about the number and URL.
        data = result.unwrap()
        url = data["html_url"]
        ui.status_err(_("created new pull request: %s\n") % url)
        number = data["number"]
        pr_id = PullRequestId(hostname=hostname, owner=owner, name=name, number=number)
        store.map_commit_to_pull_request(commit.node, pr_id)
        commits_to_update.append((commit, pr_id))

        parent = commit

    # Now that all of the pull requests have been created, update the .pr field
    # on each CommitData. We prioritize the create_pull_request() calls to try
    # to get the pull request numbers to match up.
    prs = await asyncio.gather(
        *[get_pull_request_details_or_throw(c[1]) for c in commits_to_update]
    )
    for (commit, _pr_id), pr in zip(commits_to_update, prs):
        commit.pr = pr


@dataclass
class PlaceholderStrategyParams:
    # git push --force any heads that need updating, creating new branch names,
    # if necessary.
    refs_to_update: List[str]
    pull_requests_to_create: List[PullRequestParams]
    repository: Optional[Repository]


async def create_placeholder_strategy_params(
    ui,
    partitions: List[List[CommitData]],
    github_repo: GitHubRepo,
    origin: str,
) -> PlaceholderStrategyParams:
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
                if pr.state == PullRequestState.CLOSED:
                    ui.status_err(_("%s was closed.\n") % pr.url)
                elif pr.state == PullRequestState.MERGED:
                    ui.status_err(_("%s was merged.\n") % pr.url)
                else:
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

    return PlaceholderStrategyParams(
        refs_to_update, pull_requests_to_create, repository
    )


async def create_pull_requests_from_placeholder_issues(
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

        response = await gh_submit.create_pull_request_from_placeholder_issue(
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

        if response.is_err():
            raise error.Abort(
                _("error creating pull request for %s: %s")
                % (hex(commit.node), response.unwrap_err())
            )

        # Because create_pull_request() uses the REST API instead of the
        # GraphQL API [where we would have to enumerate the fields we
        # want in the response], the response JSON appears to contain
        # "anything" we might want, but we only care about the number and URL.
        data = response.unwrap()
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

    def unwrap(r: Result[int, str]) -> int:
        if r.is_err():
            raise error.Abort(
                _(
                    "Error while trying to create a placeholder issue for a pull request on %s/%s: %s"
                )
                % (upstream_owner, upstream_name, r.unwrap_err())
            )
        else:
            return r.unwrap()

    issue_numbers = [unwrap(r) for r in issue_number_results]
    issue_numbers.sort()
    return issue_numbers


async def get_repository_for_origin(origin: str, hostname: str) -> Repository:
    origin_owner, origin_name = get_owner_and_name(origin)
    return await get_repo(hostname, origin_owner, origin_name)


def get_push_origin(ui) -> str:
    test_url = os.environ.get("SL_TEST_GH_URL")
    if test_url:
        origin = test_url
    else:
        origin = ui.expandpath("default-push", "default")
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
    repository = repo_result.ok()
    if repository:
        return repository
    else:
        raise error.Abort(_("failed to fetch repo id: %s") % repo_result.unwrap_err())


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
    if result.is_err():
        raise error.Abort(
            _("error fetching %s: %s") % (pr_id.as_url(), result.unwrap_err())
        )
    else:
        return result.unwrap()
