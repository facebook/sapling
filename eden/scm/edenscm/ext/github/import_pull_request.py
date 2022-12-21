# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import asyncio
from typing import Optional

from edenscm import error
from edenscm.hg import updatetotally
from edenscm.i18n import _
from edenscm.node import bin
from ghstack.stackheader import STACK_HEADER_PREFIX as GHSTACK_HEADER_PREFIX

from .gh_submit import get_pull_request_details
from .pull_request_arg import parse_pull_request_arg
from .pull_request_body import parse_stack_information
from .pullrequest import PullRequestId
from .pullrequeststore import PullRequestStore


def get_pr(ui, repo, *args, **opts):
    pr_arg = args[0]
    pr_id = parse_pull_request_arg(pr_arg, repo=repo)
    if pr_id is None:
        raise error.Abort(
            _("Could not parse pull request arg: '%s'. " "Specify PR by URL or number.")
            % pr_arg
        )

    is_goto = opts.get("goto")
    return asyncio.run(_get_pr(ui, repo, pr_id=pr_id, is_goto=is_goto))


async def _get_pr(ui, repo, pr_id: PullRequestId, is_goto: bool):
    """
    pull_request_arg: the identifier the user supplied for the pull request.
    """
    result = await get_pull_request_details(pr_id)
    if result.is_err():
        raise error.Abort(
            _("could not get pull request details: %s") % result.unwrap_err()
        )

    pull_request = result.unwrap()
    body = pull_request.body

    # Test body to see if it is part of a ghstack stack, in which case the user
    # probably wants to run `sl ghstack checkout <pr_id>` instead?
    if _looks_like_ghstack_pull_request(body):
        raise error.Abort(
            _(
                "This pull request appears to be part of a ghstack stack.\n"
                "Try running the following instead:\n"
                "    sl ghstack checkout %s"
            )
            % pr_id.as_url()
        )

    head_oid = pull_request.head_oid
    head_oid_node = bin(head_oid)
    repo.pull(headnodes=(head_oid_node,))
    store = PullRequestStore(repo)
    store.map_commit_to_pull_request(head_oid_node, pr_id)
    ui.status(_("imported #%d as %s\n") % (pr_id.number, head_oid))

    # Parse the body and update any other commits in the stack.
    stack_entries = parse_stack_information(body)
    current = next(filter(lambda x: x[1][0], enumerate(stack_entries)), None)
    unlinked_ancestors = []
    if current:
        index, (_is_current_entry, number) = current
        if number != pr_id.number:
            # Suspicious...
            ui.warn(
                _(
                    (
                        "__->__ in pull request body identified #%d instead of #%d.\n"
                        "Ancestors will not be linked to pull requests.\n"
                    )
                )
                % (number, pr_id.number)
            )
        else:
            ancestor_entries = stack_entries[index + 1 :]
            if ancestor_entries:
                unlinked_ancestors = list(
                    filter(
                        lambda pr: pr is not None,
                        await asyncio.gather(
                            *[
                                _link_pull_request(store, pr_id, entry[1])
                                for entry in ancestor_entries
                            ]
                        ),
                    )
                )

    else:
        ui.warn(
            _(
                "No stack information found in the pull request body.\n"
                "Ancestors will not be linked to pull requests.\n"
            )
        )

    if is_goto:
        updatetotally(ui, repo, head_oid_node, None)
        ui.status(_("now at #%d\n") % pr_id.number)

    if unlinked_ancestors:
        for pr_id in unlinked_ancestors:
            ui.status_err(_("Failed to link a commit to %s.\n") % pr_id.as_url())
        raise error.Abort(
            _(
                "Not all PRs listed in the pull request body could be linked.\n"
                "Consider `sl pr link` to fix missing links. %s"
            )
            % unlinked_ancestors
        )


async def _link_pull_request(
    store: PullRequestStore, original: PullRequestId, number: int
) -> Optional[PullRequestId]:
    """If the link fails, returns the id of the PR that could not be linked."""
    pr_id = PullRequestId(
        hostname=original.hostname,
        owner=original.owner,
        name=original.name,
        number=number,
    )
    result = await get_pull_request_details(pr_id)
    if result.is_err():
        return pr_id

    pull_request = result.unwrap()
    head_oid_node = bin(pull_request.head_oid)
    store.map_commit_to_pull_request(head_oid_node, pr_id)
    return None


def _looks_like_ghstack_pull_request(body: str) -> bool:
    for line in body.splitlines():
        if line.startswith(GHSTACK_HEADER_PREFIX):
            return True
    return False
