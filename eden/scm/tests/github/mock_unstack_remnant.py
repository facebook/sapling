# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from sapling import extensions, scmutil
from sapling.ext.github import github_gh_cli, submit
from sapling.ext.github.mock_utils import (
    mock_run_git_command,
    MockGitHubServer,
    wrap_with_consumption_check,
)

# An extension to mock network requests for `sl pr submit --restack` with
# github.pr-workflow=stacked when dissolving the diverged stack only
# partially succeeds: pull requests that are merged or queued for merge
# cannot be unstacked and are left in place. Recreating the stack is not
# possible while they remain in the old one, so the submit must abort before
# updating or pushing anything.


def setup_mock_github_server(repo) -> MockGitHubServer:
    github_server = MockGitHubServer()

    github_server.expect_get_repository_request().and_respond()

    for desc, num in [("one", 42), ("insert", 45), ("two", 43)]:
        commit = scmutil.revsingle(repo, f"desc({desc})").hex()
        github_server.expect_get_pr_details_request(num).and_respond(
            f"PR_id_{num}", head_ref_oid=commit
        )
    # #44 is stale: its commit was amended.
    github_server.expect_get_pr_details_request(44).and_respond("PR_id_44")

    # The stack on GitHub has a different order than the local stack.
    github_server.expect_get_stack_request(42).and_respond(
        stack_number=100, pr_numbers=[43, 42, 45, 44]
    )

    # Dissolving the stack leaves #43 in place (e.g., it is queued for
    # merge).
    github_server.expect_unstack_request(100).and_respond(
        remaining_pr_numbers=[43]
    )

    return github_server


def reposetup(ui, repo):
    github_server = setup_mock_github_server(repo)
    extensions.wrapfunction(
        github_gh_cli, "_make_request", github_server.make_request
    )
    extensions.wrapfunction(submit, "run_git_command", mock_run_git_command)
    wrap_with_consumption_check(github_server, submit, "submit")
