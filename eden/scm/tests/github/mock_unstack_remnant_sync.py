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

# Like mock_unstack_remnant.py, but for the case where all local commits are
# up-to-date (nothing to push): dissolving the diverged stack happens during
# the stack sync at the end of the submit, and a partial dissolution is
# reported as a warning (the stack is not recreated).


def setup_mock_github_server(repo) -> MockGitHubServer:
    github_server = MockGitHubServer()

    github_server.expect_get_repository_request().and_respond()

    for desc, num in [("one", 42), ("two", 43)]:
        commit = scmutil.revsingle(repo, f"desc({desc})").hex()
        github_server.expect_get_pr_details_request(num).and_respond(
            f"PR_id_{num}", head_ref_oid=commit
        )

    # The stack on GitHub has a different order than the local stack.
    github_server.expect_get_stack_request(42).and_respond(
        stack_number=100, pr_numbers=[43, 42]
    )

    # Dissolving the stack leaves #43 in place (e.g., it is queued for
    # merge), so the stack cannot be recreated.
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
