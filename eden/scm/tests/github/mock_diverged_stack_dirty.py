# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from sapling import extensions
from sapling.ext.github import github_gh_cli, submit
from sapling.ext.github.mock_utils import mock_run_git_command, MockGitHubServer

# An extension to mock network requests for `sl pr submit` with
# github.pr-workflow=stacked when the stack on GitHub has diverged from the
# local stack AND there are local changes to push (the top commit was
# amended).
#
# Without --restack, the submit must abort BEFORE updating any base branches
# or pushing anything: partial updates against a diverged stack can corrupt
# it. With --restack, the diverged stack is dissolved up front, the bases and
# branches are updated, and the stack is recreated from the local stack.

COMMIT_ONE = "ebe5b8faff36687becb7bdbca1e6a61dac428834"
COMMIT_TWO = "1a67244b0a776bfcc3be6bf811e98c993d78ce47"
# The commit "three" after it was amended. Its head branch (pr44) needs to be
# pushed, which makes this a "mutating" submit.
COMMIT_THREE_AMENDED = "0000000000000000000000000000000000000000"


def setup_mock_github_server(ui) -> MockGitHubServer:
    github_server = MockGitHubServer()

    github_server.expect_get_repository_request().and_respond()

    github_server.expect_get_pr_details_request(42).and_respond(
        "PR_id_42", head_ref_oid=COMMIT_ONE
    )
    github_server.expect_get_pr_details_request(43).and_respond(
        "PR_id_43", head_ref_oid=COMMIT_TWO
    )
    # PR #44's head is stale (the commit was amended), so it needs a push.
    github_server.expect_get_pr_details_request(44).and_respond(
        "PR_id_44", head_ref_oid="f4185fef85f10d46b859c30076243068b0f59245"
    )

    # The stack on GitHub has a different order than the local stack
    # (#42, #43, #44). It is queried up front, before any mutations.
    github_server.expect_get_stack_request(42).and_respond(
        stack_number=100, pr_numbers=[43, 42, 44]
    )

    # Only used with --restack: the diverged stack is dissolved up front,
    # base branches are updated, the amended commit is pushed, and the stack
    # is recreated from the local stack.
    github_server.expect_unstack_request(100).and_respond()
    github_server.expect_update_pr_request("PR_id_44", 44, "", base="pr43").and_respond()
    github_server.expect_update_pr_request("PR_id_43", 43, "", base="pr42").and_respond()
    github_server.expect_update_pr_request("PR_id_42", 42, "", base="main").and_respond()
    github_server.expect_update_pr_request(
        "PR_id_44", 44, "three\n", base=None
    ).and_respond()
    github_server.expect_update_pr_request(
        "PR_id_43", 43, "two\n", base=None
    ).and_respond()
    github_server.expect_update_pr_request(
        "PR_id_42", 42, "one\n", base=None
    ).and_respond()
    github_server.expect_get_username_request().and_respond()
    github_server.expect_merge_into_branch(COMMIT_THREE_AMENDED).and_respond()
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
