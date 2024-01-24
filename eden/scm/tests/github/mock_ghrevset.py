# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from ghstack import github_gh_cli
from sapling import extensions
from sapling.ext.github import submit
from sapling.ext.github.mock_utils import mock_run_git_command, MockGitHubServer
from sapling.ext.github.pull_request_body import firstline

# An extension to mock network requests by replacing the `github_gh_cli.make_request`
# and `submit.run_git_command` with the corresponding wrapper functions. Check `uisetup`
# function for how wrapper functions are registered.


def setup_mock_github_server():
    """Setup mock GitHub Server for testing happy case of `sl pr submit` command."""
    github_server = MockGitHubServer()

    github_server.expect_get_repository_request().and_respond()

    next_pr_number = 7
    github_server.expect_guess_next_pull_request_number().and_respond()

    body = "neobranch\n"
    title = firstline(body)
    head = f"pr{next_pr_number}"
    github_server.expect_create_pr_request(
        body=body, title=title, head=head
    ).and_respond(number=next_pr_number)

    pr_id = f"PR_id_{next_pr_number}"
    github_server.expect_get_pr_details_request(next_pr_number).and_respond(
        pr_id,
        head_ref_oid="4ce18fc3106a6f8dc65f0af182bed42275b2c3e6",
    )

    return github_server


def uisetup(ui):
    mock_github_server = setup_mock_github_server()
    extensions.wrapfunction(
        github_gh_cli, "_make_request", mock_github_server.make_request
    )
    extensions.wrapfunction(submit, "run_git_command", mock_run_git_command)
