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
# github.pr-workflow=stacked when the pull request's stack on GitHub is
# closed. A closed stack cannot be appended to or dissolved, so it is treated
# the same as no stack: the base branch is updated normally and no stack
# operations are attempted (a single open pull request cannot form a new
# stack).


def setup_mock_github_server(repo) -> MockGitHubServer:
    github_server = MockGitHubServer()

    github_server.expect_get_repository_request().and_respond()

    # #42's head is stale (the commit was amended), so it is pushed.
    github_server.expect_get_pr_details_request(42).and_respond("PR_id_42")

    # The stack containing #42 is closed.
    github_server.expect_get_stack_request(42).and_respond(
        stack_number=100, pr_numbers=[42, 43], is_open=False
    )

    # Since the closed stack does not manage #42 anymore, its base branch is
    # updated via the API.
    github_server.expect_update_pr_request(
        "PR_id_42", 42, "", base="main"
    ).and_respond()

    msg = "one\n\nPull Request resolved: https://github.com/facebook/test_github_repo/pull/42"
    github_server.expect_update_pr_request(
        "PR_id_42", 42, msg, base=None
    ).and_respond()

    github_server.expect_get_username_request().and_respond()
    tip = scmutil.revsingle(repo, "desc(one)").hex()
    github_server.expect_merge_into_branch(tip).and_respond()

    return github_server


def reposetup(ui, repo):
    github_server = setup_mock_github_server(repo)
    extensions.wrapfunction(
        github_gh_cli, "_make_request", github_server.make_request
    )
    extensions.wrapfunction(submit, "run_git_command", mock_run_git_command)
    wrap_with_consumption_check(github_server, submit, "submit")
