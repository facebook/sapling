# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Implementation of `sl pr get` command to pull entire PR stacks from GitHub.

This command is similar to `sl pr pull` but fetches an entire stack of PRs
instead of just a single PR.

Usage:
    sl pr get 123                    # Pull stack for PR #123
    sl pr get https://github.com/.../pull/123
    sl pr get 123 --goto             # Pull and checkout target
    sl pr get 123 --downstack        # Only fetch trunk→target (skip upstack)
"""

import asyncio

from sapling import error
from sapling.hg import updatetotally
from sapling.i18n import _
from sapling.node import bin

from .consts.stackheader import STACK_HEADER_PREFIX as GHSTACK_HEADER_PREFIX
from .pull_request_arg import parse_pull_request_arg
from .pullrequest import PullRequestId
from .pullrequeststore import PullRequestStore
from .stack_discovery import discover_stack_from_pr, get_head_nodes


def get(ui, repo, *args, **opts):
    """Pull an entire PR stack from GitHub.

    The PULL_REQUEST can be specified as either a URL:
    `https://github.com/facebook/sapling/pull/321`
    or just the PR number within the GitHub repository identified by
    `sl config paths.default`.

    This command differs from `sl pr pull` in that it fetches the entire
    stack of PRs, not just a single PR.
    """
    if len(args) == 0:
        raise error.Abort(
            _("PR URL or number must be specified. See '@prog@ pr get -h'.")
        )

    pr_arg = args[0]
    pr_id = parse_pull_request_arg(pr_arg, repo=repo)
    if pr_id is None:
        raise error.Abort(
            _("Could not parse pull request arg: '%s'. Specify PR by URL or number.")
            % pr_arg
        )

    is_goto = opts.get("goto")
    downstack_only = opts.get("downstack")
    use_worktree = opts.get("wt")
    wt_name = opts.get("wt_name")

    return asyncio.run(
        _get_stack(
            ui,
            repo,
            pr_id=pr_id,
            is_goto=is_goto,
            downstack_only=downstack_only,
            use_worktree=use_worktree,
            wt_name=wt_name,
        )
    )


async def _get_stack(
    ui,
    repo,
    pr_id: PullRequestId,
    is_goto: bool,
    downstack_only: bool,
    use_worktree: bool = False,
    wt_name: str = "",
) -> int:
    """Internal implementation of stack fetch.

    Returns 0 on success.
    """
    ui.status(_("discovering stack for PR #%d...\n") % pr_id.number)

    # Check for ghstack PRs which need different handling
    from .gh_submit import get_pull_request_details

    initial_result = await get_pull_request_details(pr_id)
    if initial_result.is_err():
        raise error.Abort(
            _("could not get pull request details: %s") % initial_result.unwrap_err()
        )

    initial_details = initial_result.unwrap()
    if _looks_like_ghstack_pull_request(initial_details.body):
        raise error.Abort(
            _(
                "This pull request appears to be part of a ghstack stack.\n"
                "Try running the following instead:\n"
                "    sl ghstack checkout %s"
            )
            % pr_id.as_url()
        )

    # Discover the full stack
    try:
        stack_result = await discover_stack_from_pr(pr_id, downstack_only=downstack_only)
    except RuntimeError as e:
        raise error.Abort(_("stack discovery failed: %s") % str(e))

    num_prs = len(stack_result.entries)
    if num_prs == 1:
        ui.status(_("found single PR (no stack)\n"))
    else:
        mode = "downstack" if downstack_only else "full stack"
        ui.status(_("found %d PRs in %s\n") % (num_prs, mode))

    # Get all head nodes for pulling
    head_nodes = get_head_nodes(stack_result)

    # Pull all commits
    ui.status(_("pulling %d commit(s)...\n") % len(head_nodes))
    repo.pull(headnodes=tuple(head_nodes))

    # Link all commits to their respective PRs
    store = PullRequestStore(repo)
    target_node = None

    for entry in stack_result.entries:
        node = bin(entry.head_oid)
        store.map_commit_to_pull_request(node, entry.pr_id)

        if entry.is_target:
            target_node = node

        ui.status(_("imported #%d as %s\n") % (entry.number, entry.head_oid[:12]))

    ui.status(_("successfully imported %d PR(s)\n") % num_prs)

    # Create worktree if requested (takes precedence over goto)
    if use_worktree and target_node is not None:
        from sapling.node import hex as node_hex

        from ..worktree import create_worktree_for_commit

        target_hex = node_hex(target_node)
        worktree_name = wt_name if wt_name else f"pr-{pr_id.number}"
        create_worktree_for_commit(ui, repo, target_hex, name=worktree_name)
    # Goto target if requested (and not using worktree)
    elif is_goto and target_node is not None:
        target_entry = stack_result.entries[stack_result.target_index]
        updatetotally(ui, repo, target_node, None)
        ui.status(_("now at #%d\n") % target_entry.number)

    return 0


def _looks_like_ghstack_pull_request(body: str) -> bool:
    """Check if a PR body indicates it was created by ghstack."""
    for line in body.splitlines():
        if line.startswith(GHSTACK_HEADER_PREFIX):
            return True
    return False
