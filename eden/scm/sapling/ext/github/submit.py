# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import asyncio
import os
import webbrowser
from dataclasses import dataclass
from enum import Enum
from typing import Any, List, Optional, Tuple

from sapling import error, formatter, git, hintutil, templatekw
from sapling.context import changectx
from sapling.i18n import _
from sapling.node import hex, nullid
from sapling.result import Result

from . import gh_submit, github_repo_util
from .archive_commit import add_commit_to_archives
from .gh_submit import PullRequestDetails, PullRequestState, Repository
from .github_repo_util import check_github_repo, GitHubRepo
from .none_throws import none_throws
from .pr_parser import get_pull_request_for_context
from .pull_request_body import create_pull_request_title_and_body, title_and_body
from .pullrequest import PullRequestId
from .pullrequeststore import PullRequestStore
from .run_git_command import run_git_command
from .templates import _GITHUB_PULL_REQUEST_URL_REVCACHE_KEY


def submit(ui, repo, *args, **opts) -> int:
    """Create or update GitHub pull requests."""
    github_repo = check_github_repo(repo)
    is_draft = opts.get("draft")
    is_open = opts.get("open")
    is_restack = opts.get("restack")
    return asyncio.run(
        update_commits_in_stack(
            ui,
            repo,
            github_repo,
            is_draft=is_draft,
            is_open=is_open,
            restack=is_restack,
        )
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

    """Like SINGLE, but additionally links the pull requests together using
    GitHub's native "stacked pull requests" feature so GitHub renders the
    stack natively and can merge/retarget it bottom-up:
    https://docs.github.com/en/pull-requests/get-started/about-stacked-prs
    """
    STACKED = "stacked"

    def uses_chained_bases(self) -> bool:
        """Whether each PR in the stack uses the head branch of the PR below
        it as its base branch (as opposed to all PRs sharing a common base).
        """
        return self in (SubmitWorkflow.SINGLE, SubmitWorkflow.STACKED)

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
        elif workflow == "stacked":
            return SubmitWorkflow.STACKED
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
    ui,
    repo,
    github_repo: GitHubRepo,
    is_draft: bool,
    is_open: bool = False,
    restack: bool = False,
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

    max_pull_requests_to_create = ui.configint("github", "max-prs-to-create", "5")
    if (
        max_pull_requests_to_create >= 0
        and params.pull_requests_to_create
        and len(params.pull_requests_to_create) > max_pull_requests_to_create
    ):
        raise error.Abort(
            _(
                "refused to create %d pull requests, max is %d\nif you want to create %d pull requests at once, run again with `--config github.max-prs-to-create=-1`"
            )
            % (
                len(params.pull_requests_to_create),
                max_pull_requests_to_create,
                len(params.pull_requests_to_create),
            )
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

    if not refs_to_update:
        ui.status_err(_("no pull requests to update\n"))
        if workflow == SubmitWorkflow.STACKED:
            # Even when there is nothing to push, ensure the pull requests are
            # linked into a native GitHub stack. This makes stack creation
            # idempotent: if it failed on a previous run (or the stack was
            # modified on GitHub), re-running `sl pr submit` reconciles it.
            repository = params.repository
            if not repository:
                repository = await get_repository_for_origin(
                    origin, github_repo.hostname
                )
            await sync_github_stack(ui, partitions, repository, restack=restack)
        return 0

    repository = params.repository

    # For the STACKED workflow, consult the native GitHub stack BEFORE making
    # any changes:
    #
    # - If the stack on GitHub has diverged from the local stack and --restack
    #   was not passed, abort now, before any base updates or pushes: partial
    #   updates against a diverged stack can corrupt it.
    # - With --restack, dissolve the diverged stack now, so that subsequent
    #   base branch updates are not rejected by GitHub (GitHub does not allow
    #   changing the base branch of a pull request that is in a stack).
    # - If the stack matches (or is extended by) the local stack, remember its
    #   members: their base branches are managed by the stack itself and must
    #   not be updated via the API.
    github_stack: Optional[gh_submit.StackDetails] = None
    github_stack_fetched = False
    prs_in_github_stack = set()
    if workflow == SubmitWorkflow.STACKED:
        if not repository:
            repository = await get_repository_for_origin(origin, github_repo.hostname)
        if not repository.is_fork:
            local_stack = local_open_pr_numbers(partitions)
            if local_stack:
                fetched, github_stack = await query_github_stack(
                    ui, repository, local_stack
                )
                if not fetched:
                    # Fail closed: without knowing the state of the stack on
                    # GitHub, base updates and pushes could corrupt it. (The
                    # warning with the underlying error was already printed.)
                    raise error.Abort(
                        _(
                            "could not determine the state of the stack on "
                            "GitHub; re-run 'pr submit' to retry"
                        )
                    )
                github_stack_fetched = True
                if github_stack:
                    stack_prs = github_stack.pull_requests
                    new_pr_inserted = _has_new_pr_below_stack_top(
                        partitions, set(stack_prs)
                    )
                    if stack_prs == local_stack[: len(stack_prs)] and not new_pr_inserted:
                        # The stack matches the local stack (possibly extended
                        # at the top with new pull requests).
                        prs_in_github_stack = set(stack_prs)
                    elif restack:
                        # Dissolving the stack is only useful if it can be
                        # recreated afterwards, which requires at least two
                        # open pull requests in the local stack.
                        num_new_prs = sum(1 for p in partitions if not p[0].pr)
                        if len(local_stack) + num_new_prs < 2:
                            raise error.Abort(
                                _(
                                    "--restack would leave stack #%d with "
                                    "fewer than two pull requests; dissolve "
                                    "it on GitHub instead if that is intended"
                                )
                                % github_stack.number
                            )
                        # The stacks API has no "reorder" operation, so
                        # dissolve the diverged stack now; it is recreated
                        # from the local stack at the end of the submit.
                        unstack_result = await gh_submit.unstack(
                            repository.hostname,
                            *repository.get_upstream_owner_and_name(),
                            github_stack.number,
                        )
                        if unstack_result.is_err():
                            raise error.Abort(
                                _("failed to dissolve stack #%d: %s")
                                % (github_stack.number, unstack_result.unwrap_err())
                            )
                        # Merged or queued pull requests cannot be unstacked
                        # and are left in place; recreating the stack is not
                        # possible while they remain in the old one.
                        remnant = unstack_result.unwrap()
                        if remnant and remnant.pull_requests:
                            raise error.Abort(
                                _(
                                    "stack #%d was only partially dissolved: "
                                    "%s could not be unstacked (merged or "
                                    "queued pull requests are left in place)"
                                )
                                % (remnant.number, _pr_list(remnant.pull_requests))
                            )
                        github_stack = None
                    else:
                        if new_pr_inserted:
                            ui.status_err(
                                _(
                                    "new pull requests would be inserted below "
                                    "the top of stack #%d on GitHub (%s), which "
                                    "requires recreating the stack; not "
                                    "updating it\n"
                                )
                                % (
                                    github_stack.number,
                                    _pr_list(github_stack.pull_requests),
                                )
                            )
                        else:
                            ui.status_err(
                                _(
                                    "stack #%d on GitHub (%s) does not match "
                                    "your local stack (%s); not updating it\n"
                                )
                                % (
                                    github_stack.number,
                                    _pr_list(github_stack.pull_requests),
                                    _pr_list(local_stack),
                                )
                            )
                        hintutil.triggershow(ui, "pr-submit-restack")
                        raise error.Abort(
                            _("stack on GitHub has diverged from your local stack")
                        )

    # For workflows with chained base branches (SINGLE, STACKED), we must
    # update the base branch on existing PRs BEFORE pushing the new branch
    # contents. Otherwise, when commits are reordered in the stack, GitHub may
    # see that a PR's commits already exist in its (old) base branch and
    # auto-close the PR as "merged".
    #
    # See https://github.com/facebook/sapling/issues/1275
    if workflow.uses_chained_bases():
        existing_prs = [
            p for p in partitions if p[0].pr and p[0].pr.state == PullRequestState.OPEN
        ]
        if existing_prs:
            if not repository:
                repository = await get_repository_for_origin(
                    origin, github_repo.hostname
                )
            # Update base branches on existing PRs before pushing.
            # Process from bottom of stack to top so bases are set correctly.
            for index in range(len(partitions)):
                partition = partitions[index]
                pr = partition[0].pr
                if not pr or pr.state != PullRequestState.OPEN:
                    continue
                if pr.number in prs_in_github_stack:
                    # GitHub rejects base branch changes for pull requests
                    # that are in a native stack; the stack manages base
                    # branches itself.
                    continue
                base = repository.get_base_branch()
                # Chain to the nearest partition below whose pull request is
                # open (or that will get a new pull request). Closed/merged
                # pull requests are skipped: using their head branches as
                # bases would break the chain.
                for below in partitions[index + 1 :]:
                    below_head = below[0]
                    below_pr = below_head.pr
                    if below_pr and below_pr.state != PullRequestState.OPEN:
                        continue
                    base = none_throws(below_head.head_branch_name)
                    break
                result = await gh_submit.update_pull_request(
                    repository.hostname, pr.node_id, pr.title, pr.body, base
                )
                if result.is_err():
                    ui.status_err(
                        _("warning, updating base for #%d may not have succeeded: %s\n")
                        % (pr.number, result.unwrap_err())
                    )
                else:
                    ui.status_err(_("updated base for %s\n") % pr.url)

    gitdir = get_gitdir()
    git_push_args = ["push", "--force", origin] + refs_to_update
    ui.status_err(_("pushing %d to %s\n") % (len(refs_to_update), origin))
    run_git_command(git_push_args, gitdir)

    if params.pull_requests_to_create:
        if not repository:
            repository = await get_repository_for_origin(origin, github_repo.hostname)
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

    if workflow == SubmitWorkflow.STACKED:
        await sync_github_stack(
            ui,
            partitions,
            repository,
            restack=restack,
            stack=github_stack,
            stack_fetched=github_stack_fetched,
        )

    # Open pull requests in browser if --open flag was specified
    if is_open:
        pr_urls = [none_throws(p[0].pr).url for p in partitions if p[0].pr]
        for url in pr_urls:
            ui.status_err(_("opening %s\n") % url)
            webbrowser.open(url)

    return 0


def local_open_pr_numbers(partitions: List[List[CommitData]]) -> List[int]:
    """Numbers of the open pull requests in the local stack, ordered from the
    bottom of the stack to the top (as expected by the GitHub stacks API).
    Note that `partitions` is ordered from the top of the stack to the bottom.
    """
    return [
        none_throws(p[0].pr).number
        for p in reversed(partitions)
        if p[0].pr and p[0].pr.state == PullRequestState.OPEN
    ]


def _pr_list(numbers: List[int]) -> str:
    return ", ".join(f"#{n}" for n in numbers)


def _has_new_pr_below_stack_top(
    partitions: List[List[CommitData]], stack_prs: set
) -> bool:
    """True if a commit without a pull request (i.e. one that will get a new
    pull request) sits below a member of the native GitHub stack. Appending
    to a stack is only possible at the top, so this requires recreating the
    stack even when the existing members are otherwise in order.
    """
    seen_new = False
    # `partitions` is ordered from the top of the stack to the bottom.
    for p in reversed(partitions):
        head = p[0]
        pr = head.pr
        if pr is None:
            seen_new = True
        elif (
            seen_new
            and pr.state == PullRequestState.OPEN
            and pr.number in stack_prs
        ):
            return True
    return False


async def query_github_stack(
    ui, repository: Repository, local: List[int]
) -> Tuple[bool, Optional["gh_submit.StackDetails"]]:
    """Queries GitHub for the native stack containing the local stack's pull
    requests, if any. Returns (fetched, stack) where `fetched` is False if the
    query failed (a warning is printed in that case).
    """
    owner, name = repository.get_upstream_owner_and_name()
    hostname = repository.hostname

    # Query with the bottom pull request first: it is the most stable member
    # of an existing stack. Also try the top one to catch the case where the
    # local stack was extended (or reordered) at the bottom.
    stack = None
    for number in dict.fromkeys([local[0], local[-1]]):
        result = await gh_submit.get_stack_for_pull_request(
            hostname, owner, name, number
        )
        if result.is_err():
            ui.status_err(
                _("warning, could not query stacks for #%d: %s\n")
                % (number, result.unwrap_err())
            )
            return False, None
        stack = result.unwrap()
        if stack:
            break
    if stack and not stack.is_open:
        # A closed stack cannot be appended to or dissolved; treat it the
        # same as no stack.
        stack = None
    return True, stack


async def sync_github_stack(
    ui,
    partitions: List[List[CommitData]],
    repository: Repository,
    restack: bool = False,
    stack: Optional["gh_submit.StackDetails"] = None,
    stack_fetched: bool = False,
) -> None:
    """Links the pull requests together using GitHub's native "stacked pull
    requests" feature.

    Creates the stack if it does not exist and appends new pull requests to
    the top of an existing one. If the stack on GitHub has diverged from the
    local stack (e.g., commits were reordered or removed), no changes are made
    to it unless `restack` is True, in which case the stack on GitHub is
    dissolved and recreated to match the local stack.

    Failures to sync the stack are reported as warnings rather than aborting:
    at this point, the pull requests themselves have already been created or
    updated successfully, and re-running `sl pr submit` will retry linking.
    """
    if repository.is_fork:
        # GitHub requires all branches of a stack to be in the same
        # repository, so stacks are not supported across forks.
        ui.status_err(
            _(
                "warning: GitHub does not support stacks across forks; "
                "pull requests were submitted without a stack\n"
            )
        )
        return

    local = local_open_pr_numbers(partitions)
    if len(local) < 2:
        # A stack must contain at least two pull requests. Note that GitHub
        # automatically removes merged pull requests from existing stacks, so
        # there is nothing to clean up here as the stack shrinks.
        return

    owner, name = repository.get_upstream_owner_and_name()
    hostname = repository.hostname
    pr_list = _pr_list

    if not stack_fetched:
        fetched, stack = await query_github_stack(ui, repository, local)
        if not fetched:
            return

    if stack is None:
        result = await gh_submit.create_stack(hostname, owner, name, local)
        if result.is_err():
            ui.status_err(
                _("warning, failed to create stack for %s: %s\n")
                % (pr_list(local), result.unwrap_err())
            )
        else:
            ui.status_err(_("created stack: %s\n") % result.unwrap().url)
    elif stack.pull_requests == local:
        ui.status_err(_("stack #%d is up-to-date\n") % stack.number)
    elif stack.pull_requests == local[: len(stack.pull_requests)]:
        # The local stack extends the stack on GitHub at the top, so the new
        # pull requests can simply be appended.
        to_add = local[len(stack.pull_requests) :]
        result = await gh_submit.add_prs_to_stack(
            hostname, owner, name, stack.number, to_add
        )
        if result.is_err():
            ui.status_err(
                _("warning, failed to add %s to stack #%d: %s\n")
                % (pr_list(to_add), stack.number, result.unwrap_err())
            )
        else:
            ui.status_err(
                _("added %s to stack #%d\n") % (pr_list(to_add), stack.number)
            )
    elif restack:
        # The stacks API has no "reorder" operation, so dissolve the stack and
        # recreate it from the local stack.
        unstack_result = await gh_submit.unstack(hostname, owner, name, stack.number)
        if unstack_result.is_err():
            ui.status_err(
                _("warning, failed to dissolve stack #%d: %s\n")
                % (stack.number, unstack_result.unwrap_err())
            )
            return
        # Merged or queued pull requests cannot be unstacked and are left in
        # place; recreating the stack is not possible while they remain in
        # the old one.
        remnant = unstack_result.unwrap()
        if remnant and remnant.pull_requests:
            ui.status_err(
                _(
                    "warning, stack #%d was only partially dissolved: %s "
                    "could not be unstacked (merged or queued pull requests "
                    "are left in place)\n"
                )
                % (remnant.number, _pr_list(remnant.pull_requests))
            )
            return
        result = await gh_submit.create_stack(hostname, owner, name, local)
        if result.is_err():
            ui.status_err(
                _("warning, failed to recreate stack for %s: %s\n")
                % (pr_list(local), result.unwrap_err())
            )
        else:
            ui.status_err(_("recreated stack: %s\n") % result.unwrap().url)
    else:
        ui.status_err(
            _(
                "warning: stack #%d on GitHub (%s) does not match your local "
                "stack (%s); not updating it\n"
            )
            % (stack.number, pr_list(stack.pull_requests), pr_list(local))
        )
        hintutil.triggershow(ui, "pr-submit-restack")


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
    base: Optional[str] = repository.get_base_branch()
    if workflow == SubmitWorkflow.STACKED:
        # GitHub rejects base branch changes for pull requests that are in a
        # native stack (the stack manages base branches itself), so leave the
        # base untouched when updating the title/body. New pull requests get
        # the correct base at creation time.
        base = None
    elif workflow.uses_chained_bases() and not repository.is_fork:
        # Chain to the nearest partition below whose pull request is open (or
        # new). Closed/merged pull requests are skipped: using their head
        # branches as bases would break the chain. For forks, chained bases
        # are not possible at all (the head branches live on the fork, but a
        # base branch must be a branch on the upstream repository), so the
        # default base branch is kept.
        for below in partitions[index + 1 :]:
            below_pr = below[0].pr
            if below_pr and below_pr.state != PullRequestState.OPEN:
                continue
            base = none_throws(below[0].head_branch_name)
            break

    head_commit_data = partition[0]

    pr = head_commit_data.pr
    assert pr

    title = None
    if ui.configbool("github", "preserve-pull-request-description"):
        commit_msg_or_title_body = (pr.title, pr.body)
    else:
        commit_msg_or_title_body = head_commit_data.get_msg()

    title, body = create_pull_request_title_and_body(
        commit_msg_or_title_body,
        pr_numbers_and_num_commits,
        index,
        repository,
        reviewstack=ui.configbool("github", "pull-request-include-reviewstack"),
        # For the native "stacked" workflow, GitHub renders the stack in the
        # pull request UI itself, so the footer stack list (and ReviewStack
        # link) would be redundant.
        stack_list=workflow != SubmitWorkflow.STACKED,
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
    # The first str in the Tuple is the head branch name for the commit. The
    # second is the head branch name of the partition below it in the stack
    # (None if it is at the bottom), to be used as the base branch in
    # workflows with chained bases.
    pull_requests_to_create: List[Tuple[CommitData, str, Optional[str]]]
    repository: Optional[Repository]


def get_pr_branch_name(
    ui, ctx: changectx, upstream_repository: Repository, pull_request_number: int
) -> str:
    template = ui.config(
        "github",
        "pr.branch-name-template",
        "pr{github_pull_request_number}",
    )
    tmpl = formatter.maketemplater(ui, template)

    props = templatekw.keywords.copy()
    props["templ"] = tmpl
    props["ctx"] = ctx
    props["repo"] = ctx.repo()
    props["cache"] = {}
    # In order to support {github_pull_request_number} etc., we need to inject
    # artificial pull-request info, since the repo doesn't yet have a real link
    # between the commit and the PR.
    props["revcache"] = {
        _GITHUB_PULL_REQUEST_URL_REVCACHE_KEY: PullRequestId(
            hostname=upstream_repository.hostname,
            owner=upstream_repository.owner,
            name=upstream_repository.name,
            number=pull_request_number,
        )
    }
    branch_name = tmpl.render(props)
    return branch_name


async def create_serial_strategy_params(
    ui,
    partitions: List[List[CommitData]],
    github_repo: GitHubRepo,
    origin: str,
) -> SerialStrategyParams:
    # git push --force any heads that need updating, creating new branch names,
    # if necessary.
    refs_to_update = []
    pull_requests_to_create: List[Tuple[CommitData, str, Optional[str]]] = []

    # These are set lazily because they require GraphQL calls.
    next_pull_request_number = None
    repository: Optional[Repository] = None

    # Note that `partitions` is ordered from the top of the stack to the bottom,
    # but we want to create PRs from the bottom to the top so the PR numbers are
    # created in ascending order.
    # Head branch of the partition below the current one, i.e. the base branch
    # to use for a new pull request in workflows with chained bases.
    below_branch: Optional[str] = None
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
            branch_name = get_pr_branch_name(
                ui=ui,
                ctx=top.ctx,
                upstream_repository=repository,
                pull_request_number=next_pull_request_number,
            )
            refs_to_update.append(f"{hex(top.node)}:refs/heads/{branch_name}")
            top.head_branch_name = branch_name
            pull_requests_to_create.append((top, branch_name, below_branch))
        if not pr or pr.state == PullRequestState.OPEN:
            # Only open pull requests (or commits that will get a new pull
            # request) can serve as the base for the partition above:
            # closed/merged head branches would break the chain.
            below_branch = top.head_branch_name

    return SerialStrategyParams(refs_to_update, pull_requests_to_create, repository)


def get_pull_request_template(commit: CommitData) -> None | str:
    ctx = commit.ctx
    for path in [
        ".github/pull_request_template.md",
        "docs/pull_request_template.md",
        "pull_request_template.md",
    ]:
        if path in ctx:
            fctx = ctx[path]
            data = fctx.data()
            return data.decode(errors="replace")

    return None


async def create_pull_requests_serially(
    commits: List[Tuple[CommitData, str, Optional[str]]],
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
    for commit, branch_name, below_branch in commits:
        base = repository.get_base_branch()
        if workflow.uses_chained_bases() and below_branch and not repository.is_fork:
            # Use the head branch of the partition below this commit as the
            # base branch, even if that pull request already existed. This is
            # required for the STACKED workflow (the base branch cannot be
            # corrected afterwards: GitHub rejects base branch changes for
            # pull requests in a native stack) and avoids a redundant base
            # update for the SINGLE workflow.
            #
            # For forks this is not possible: the head branches live on the
            # fork, but the base branch of a pull request must be a branch on
            # the upstream repository, so fall back to the default base
            # branch.
            base = below_branch

        commit_msg = commit.get_msg()
        title, body = title_and_body(commit_msg)
        pull_request_template = get_pull_request_template(commit)
        body = "\n\n".join(filter(None, [body, pull_request_template]))
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
        if not pr or pr.state == PullRequestState.OPEN:
            # Only open pull requests (or commits that will get a new pull
            # request) can serve as the parent for chained bases:
            # closed/merged head branches would break the chain.
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
            commit = commit_needs_pr.commit
            branch_name = get_pr_branch_name(
                ui=ui,
                ctx=commit.ctx,
                upstream_repository=repository,
                pull_request_number=number,
            )
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
        # For forks, chained bases are not possible either: the head branches
        # live on the fork, but the base branch of a pull request must be a
        # branch on the upstream repository.
        base = base_branch_for_repo
        if workflow.uses_chained_bases() and not repository.is_fork:
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
    pr_id = get_pull_request_for_context(store, repo, ctx)
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
