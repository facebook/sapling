#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import unittest
from unittest.mock import AsyncMock, Mock, call, patch

from sapling.ext.github import submit
from sapling.ext.github.gh_submit import PullRequestDetails, PullRequestState
from sapling.result import Ok


def commit_with_pr(number: int, *, is_draft: bool) -> submit.CommitData:
    pull_request = PullRequestDetails(
        node_id=f"PR_{number}",
        number=number,
        url=f"https://github.com/owner/repo/pull/{number}",
        base_oid="base",
        base_branch_name="main",
        head_oid=f"head-{number}",
        head_branch_name=f"pr{number}",
        body="",
        title=f"Pull request {number}",
        state=PullRequestState.OPEN,
        is_draft=is_draft,
    )
    return submit.CommitData(
        node=bytes([number]),
        head_branch_name=f"pr{number}",
        pr=pull_request,
        ctx=None,
        is_dep=False,
    )


class PullRequestDraftStateTest(unittest.IsolatedAsyncioTestCase):
    @patch(
        "sapling.ext.github.submit.gh_submit.set_pull_request_draft_state",
        new_callable=AsyncMock,
    )
    async def test_selected_stack_submission_preserves_ready_ancestors(self, set_state):
        set_state.return_value = Ok({})
        selected = commit_with_pr(12, is_draft=False)
        ready_ancestor = commit_with_pr(11, is_draft=False)

        updated = await submit.update_pull_request_draft_states(
            [[selected], [ready_ancestor]],
            True,
            "github.com",
            Mock(),
            preserve_ancestor_states=True,
        )

        self.assertTrue(updated)
        set_state.assert_awaited_once_with("github.com", "PR_12", True)

    @patch(
        "sapling.ext.github.submit.gh_submit.set_pull_request_draft_state",
        new_callable=AsyncMock,
    )
    async def test_complete_stack_submission_updates_every_pull_request(self, set_state):
        set_state.return_value = Ok({})
        selected = commit_with_pr(12, is_draft=False)
        ready_ancestor = commit_with_pr(11, is_draft=False)

        await submit.update_pull_request_draft_states(
            [[selected], [ready_ancestor]], True, "github.com", Mock()
        )

        self.assertEqual(
            set_state.await_args_list,
            [
                call("github.com", "PR_12", True),
                call("github.com", "PR_11", True),
            ],
        )


if __name__ == "__main__":
    unittest.main()
