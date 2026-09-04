# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from sapling import extensions
from sapling.ext.github import github_gh_cli, submit
from sapling.ext.github.mock_utils import (
    mock_run_git_command,
    MockGitHubServer,
    wrap_with_consumption_check,
)

# An extension to mock network requests for `sl pr submit` with
# github.pr-workflow=stacked when the local stack contains a single open pull
# request (#42) while the stack on GitHub contains two (#42, #43): the local
# and GitHub stacks have diverged, and --restack must refuse to dissolve the
# stack because it could not be recreated afterwards (a stack requires at
# least two pull requests).


def setup_mock_github_server(repo) -> MockGitHubServer:
    github_server = MockGitHubServer()

    github_server.expect_get_repository_request().and_respond()

    # #42's head is stale (the commit was amended), so there is something to
    # push, which makes this a "mutating" submit.
    github_server.expect_get_pr_details_request(42).and_respond("PR_id_42")

    github_server.expect_get_stack_request(42).and_respond(
        stack_number=100, pr_numbers=[42, 43]
    )

    return github_server


def reposetup(ui, repo):
    github_server = setup_mock_github_server(repo)
    extensions.wrapfunction(
        github_gh_cli, "_make_request", github_server.make_request
    )
    extensions.wrapfunction(submit, "run_git_command", mock_run_git_command)
    wrap_with_consumption_check(github_server, submit, "submit")
