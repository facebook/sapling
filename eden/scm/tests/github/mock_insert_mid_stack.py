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

# An extension to mock network requests for `sl pr submit` with
# github.pr-workflow=stacked when a new commit was inserted into the middle
# of a stack whose existing pull requests (#42, #43, #44) are linked into
# native stack #100 on GitHub in the same order.
#
# Even though the existing pull requests match the stack on GitHub, the new
# pull request cannot be appended (stacks can only grow at the top), so
# without --restack the submit must abort before updating or pushing
# anything. (See mock_insert_mid_stack_restack.py for the --restack case.)


def setup_mock_github_server(repo) -> MockGitHubServer:
    github_server = MockGitHubServer()

    github_server.expect_get_repository_request().and_respond()

    commit_one = scmutil.revsingle(repo, "desc(one)").hex()
    github_server.expect_get_pr_details_request(42).and_respond(
        "PR_id_42", head_ref_oid=commit_one
    )
    # #43 and #44 are stale: their commits were rebased on top of the
    # inserted commit.
    github_server.expect_get_pr_details_request(43).and_respond("PR_id_43")
    github_server.expect_get_pr_details_request(44).and_respond("PR_id_44")

    # The next PR number would be 45 (for the inserted commit).
    github_server.expect_guess_next_pull_request_number().and_respond(
        latest_issue_num=40, latest_pr_num=44
    )

    # The stack matches the existing pull requests, but the new pull request
    # would be inserted below its top, which requires recreating the stack.
    github_server.expect_get_stack_request(42).and_respond(
        stack_number=100, pr_numbers=[42, 43, 44]
    )

    return github_server


def reposetup(ui, repo):
    github_server = setup_mock_github_server(repo)
    extensions.wrapfunction(
        github_gh_cli, "_make_request", github_server.make_request
    )
    extensions.wrapfunction(submit, "run_git_command", mock_run_git_command)
    wrap_with_consumption_check(github_server, submit, "submit")
