# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from sapling import extensions
from sapling.ext.github import github_gh_cli, submit
from sapling.ext.github.mock_utils import mock_run_git_command, MockGitHubServer

# An extension to mock network requests by replacing the `github_gh_cli.make_request`
# and `submit.run_git_command` with the corresponding wrapper functions. Check `uisetup`
# function for how wrapper functions are registered.


def setup_mock_github_server() -> MockGitHubServer:
    """Setup mock GitHub Server for testing happy case of `sl pr submit` command."""
    github_server = MockGitHubServer()

    github_server.expect_get_repository_request().and_respond()

    pr_number = 1
    github_server.expect_create_pr_placeholder_request().and_respond(
        start_number=pr_number, num_times=1
    )

    # Body is empty because "addfile" is just the title (no body after first line)
    initial_body = ""
    github_server.expect_create_pr_using_placeholder_request(
        body=initial_body, issue=pr_number
    ).and_respond()

    pr_id = f"PR_id_{pr_number}"
    github_server.expect_get_pr_details_request(pr_number).and_respond(pr_id)

    # The update request uses the full commit message for title/body extraction
    commit_msg = "addfile\n"
    github_server.expect_update_pr_request(pr_id, pr_number, commit_msg).and_respond()
    github_server.expect_get_username_request().and_respond()

    head = "3a120a3a153f7d2960967ce6f1d52698a4d3a436"
    github_server.expect_merge_into_branch(head).and_respond()

    return github_server


def uisetup(ui):
    mock_github_server = setup_mock_github_server()
    extensions.wrapfunction(
        github_gh_cli, "_make_request", mock_github_server.make_request
    )
    extensions.wrapfunction(submit, "run_git_command", mock_run_git_command)
