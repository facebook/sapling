# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from sapling import extensions
from sapling.ext.github import github_gh_cli, submit
from sapling.ext.github.mock_utils import mock_run_git_command, MockGitHubServer

# An extension to mock network requests for `sl pr submit` with
# github.pr-workflow=stacked when a new commit is added on top of a stack
# whose pull requests (#42, #43) were created by a previous submit (see
# mock_create_stacked_prs.py): the new PR #44 should be appended to the
# existing stack on GitHub.

COMMIT_ONE = "ebe5b8faff36687becb7bdbca1e6a61dac428834"
COMMIT_TWO = "1a67244b0a776bfcc3be6bf811e98c993d78ce47"
COMMIT_THREE = "f4185fef85f10d46b859c30076243068b0f59245"


def setup_mock_github_server(ui) -> MockGitHubServer:
    github_server = MockGitHubServer()

    github_server.expect_get_repository_request().and_respond()

    # PRs #42 and #43 already exist and are up-to-date: their head OIDs match
    # the local commits, so only the new commit is pushed.
    github_server.expect_get_pr_details_request(42).and_respond(
        "PR_id_42", head_ref_oid=COMMIT_ONE
    )
    github_server.expect_get_pr_details_request(43).and_respond(
        "PR_id_43", head_ref_oid=COMMIT_TWO
    )

    # The next PR number should be 44.
    github_server.expect_guess_next_pull_request_number().and_respond(
        latest_issue_num=40, latest_pr_num=43
    )

    # Existing PRs #42 and #43 are already members of native stack #100 (see
    # the expect_get_stack_request below), so their base branches must NOT be
    # updated before pushing: GitHub rejects base branch changes for pull
    # requests that are in a stack (the stack manages bases itself). Hence
    # there are no base-update expectations here.

    # The new PR is created directly against the head branch of the pull
    # request below it in the stack: the base cannot be corrected afterwards,
    # since GitHub rejects base branch changes for PRs in a native stack.
    github_server.expect_create_pr_request(
        body="", title="three", head="pr44", base="pr43"
    ).and_respond(number=44)
    github_server.expect_get_pr_details_request(44).and_respond("PR_id_44")

    # Body rewrites leave the base branch untouched (base=None) in the
    # "stacked" workflow.
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
    github_server.expect_merge_into_branch(COMMIT_THREE).and_respond()

    # #42 is already the bottom of stack #100, and the local stack extends it
    # at the top, so #44 is appended to the existing stack. The stack is
    # queried up front (before any base updates or pushes).
    github_server.expect_get_stack_request(42).and_respond(
        stack_number=100, pr_numbers=[42, 43]
    )
    github_server.expect_add_to_stack_request(100, [44]).and_respond(
        pr_numbers=[42, 43, 44]
    )

    return github_server


def uisetup(ui):
    mock_github_server = setup_mock_github_server(ui)
    extensions.wrapfunction(
        github_gh_cli, "_make_request", mock_github_server.make_request
    )
    extensions.wrapfunction(submit, "run_git_command", mock_run_git_command)
