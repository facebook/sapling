# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from ghstack import github_gh_cli
from sapling import extensions
from sapling.ext.github import submit
from sapling.ext.github.gh_submit import PullRequestState
from sapling.ext.github.mock_utils import mock_run_git_command, MockGitHubServer


def setup_mock_github_server(ui) -> MockGitHubServer:
    github_server = MockGitHubServer()

    github_server.expect_get_repository_request().and_respond()

    pr_num = 42

    github_server.expect_get_pr_details_request(pr_num).and_respond(
        f"PR_id_{pr_num}", state=PullRequestState.CLOSED
    )

    github_server.expect_get_username_request().and_respond()

    head = "782e8bee814295e5a3d41eee2e9d823ea83a7c13"
    github_server.expect_merge_into_branch(head).and_respond()

    return github_server


def uisetup(ui):
    mock_github_server = setup_mock_github_server(ui)
    extensions.wrapfunction(
        github_gh_cli, "_make_request", mock_github_server.make_request
    )
    extensions.wrapfunction(submit, "run_git_command", mock_run_git_command)
