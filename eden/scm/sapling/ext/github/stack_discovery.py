# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Stack discovery logic for `sl pr get` command.

This module discovers the full stack of PRs from a given PR's body by parsing
the Sapling footer format.

Stack ordering:
- The stack list in PR bodies is **top-to-bottom**
- First entry = top of stack (newest commits)
- Last entry = closest to trunk (oldest commits)
"""

import asyncio
from dataclasses import dataclass
from typing import List, Optional

from .gh_submit import get_pull_request_details, PullRequestDetails
from .pull_request_body import parse_stack_information
from .pullrequest import PullRequestId


@dataclass
class StackEntry:
    """Represents a single PR in the stack."""

    pr_id: PullRequestId
    """The pull request identifier."""

    number: int
    """The PR number."""

    head_oid: str
    """The head commit OID for this PR."""

    is_target: bool
    """True if this is the target PR that the user requested."""

    details: PullRequestDetails
    """The full PR details from GitHub."""


@dataclass
class StackDiscoveryResult:
    """Result of stack discovery."""

    entries: List[StackEntry]
    """Ordered list of stack entries (top-to-bottom, first = top of stack)."""

    target_index: int
    """Index of the target PR in the entries list."""


async def discover_stack_from_pr(
    pr_id: PullRequestId,
    downstack_only: bool = False,
) -> StackDiscoveryResult:
    """Discover the full stack from a PR's body.

    Args:
        pr_id: The target PR to start discovery from.
        downstack_only: If True, only fetch PRs from target towards trunk
                       (target index to end of list).

    Returns:
        StackDiscoveryResult with ordered entries and target index.

    The stack is ordered top-to-bottom as it appears in the PR body:
    - entries[0] is the top of the stack (newest commits)
    - entries[-1] is closest to trunk (oldest commits)
    """
    # First, get the target PR details
    result = await get_pull_request_details(pr_id)
    if result.is_err():
        raise RuntimeError(f"could not get pull request details: {result.unwrap_err()}")

    target_details = result.unwrap()
    body = target_details.body

    # Parse stack information from the PR body
    stack_entries = parse_stack_information(body)

    if not stack_entries:
        # No stack info in body - treat as single-PR stack
        entry = StackEntry(
            pr_id=pr_id,
            number=pr_id.number,
            head_oid=target_details.head_oid,
            is_target=True,
            details=target_details,
        )
        return StackDiscoveryResult(entries=[entry], target_index=0)

    # Find the target PR in the stack list
    # Stack entries are List[Tuple[is_current: bool, number: int]]
    target_index = None
    for i, (is_current, number) in enumerate(stack_entries):
        if is_current or number == pr_id.number:
            target_index = i
            break

    if target_index is None:
        # Target not found in stack - maybe stack info is stale
        # Fall back to single-PR behavior
        entry = StackEntry(
            pr_id=pr_id,
            number=pr_id.number,
            head_oid=target_details.head_oid,
            is_target=True,
            details=target_details,
        )
        return StackDiscoveryResult(entries=[entry], target_index=0)

    # Determine which PRs to fetch based on downstack_only flag
    if downstack_only:
        # Fetch from target index to end (towards trunk)
        entries_to_fetch = stack_entries[target_index:]
        adjusted_target_index = 0
    else:
        # Fetch all entries
        entries_to_fetch = stack_entries
        adjusted_target_index = target_index

    # Fetch all PR details in parallel
    async def fetch_entry(index: int, is_current: bool, number: int) -> StackEntry:
        entry_pr_id = PullRequestId(
            hostname=pr_id.hostname,
            owner=pr_id.owner,
            name=pr_id.name,
            number=number,
        )

        # Use cached details for target PR
        if number == pr_id.number:
            details = target_details
        else:
            result = await get_pull_request_details(entry_pr_id)
            if result.is_err():
                raise RuntimeError(
                    f"could not get details for PR #{number}: {result.unwrap_err()}"
                )
            details = result.unwrap()

        is_target = index == adjusted_target_index if downstack_only else is_current or number == pr_id.number

        return StackEntry(
            pr_id=entry_pr_id,
            number=number,
            head_oid=details.head_oid,
            is_target=is_target,
            details=details,
        )

    entries = await asyncio.gather(
        *[
            fetch_entry(i, is_current, number)
            for i, (is_current, number) in enumerate(entries_to_fetch)
        ]
    )

    return StackDiscoveryResult(entries=list(entries), target_index=adjusted_target_index)


def get_head_nodes(result: StackDiscoveryResult) -> List[bytes]:
    """Get the head commit nodes for all entries in the stack.

    Returns:
        List of binary node IDs for all stack entry head commits.
    """
    from sapling.node import bin

    return [bin(entry.head_oid) for entry in result.entries]
