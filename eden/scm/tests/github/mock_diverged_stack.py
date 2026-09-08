# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from sapling import extensions
from sapling.ext.github import github_gh_cli, submit
from sapling.ext.github.mock_utils import mock_run_git_command, MockGitHubServer

# An extension to mock network requests for `sl pr submit` with
# github.pr-workflow=stacked when the stack on GitHub has diverged from the
# local stack (e.g., it was reordered on GitHub). All local commits are
# up-to-date, so nothing is pushed.
#
# Without --restack, the diverged stack must not be modified (a warning is
# printed instead). With --restack, the stack is dissolved and recreated, so
# this mock also includes the unstack/create expectations (they are simply
# unused in the no---restack case).

COMMIT_ONE = "ebe5b8faff36687becb7bdbca1e6a61dac428834"
COMMIT_TWO = "1a67244b0a776bfcc3be6bf811e98c993d78ce47"
COMMIT_THREE = "f4185fef85f10d46b859c30076243068b0f59245"


def setup_mock_github_server(ui) -> MockGitHubServer:
    github_server = MockGitHubServer()

    github_server.expect_get_repository_request().and_respond()

    for num, oid in [(42, COMMIT_ONE), (43, COMMIT_TWO), (44, COMMIT_THREE)]:
        github_server.expect_get_pr_details_request(num).and_respond(
            f"PR_id_{num}", head_ref_oid=oid
        )

    # The stack on GitHub has a different order than the local stack
    # (#42, #43, #44).
    github_server.expect_get_stack_request(42).and_respond(
        stack_number=100, pr_numbers=[43, 42, 44]
    )

    # Only used with --restack: the diverged stack is dissolved (204) and
    # recreated to match the local stack.
    github_server.expect_unstack_request(100).and_respond()
    github_server.expect_create_stack_request([42, 43, 44]).and_respond(
        stack_number=101
    )

    return github_server


def uisetup(ui):
    mock_github_server = setup_mock_github_server(ui)
    extensions.wrapfunction(
        github_gh_cli, "_make_request", mock_github_server.make_request
    )
    extensions.wrapfunction(submit, "run_git_command", mock_run_git_command)
