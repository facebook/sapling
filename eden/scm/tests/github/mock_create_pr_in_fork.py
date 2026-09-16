# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from sapling import extensions
from sapling.ext.github import github_gh_cli, submit
from sapling.ext.github.mock_utils import mock_run_git_command, MockGitHubServer
from sapling.ext.github.pull_request_body import title_and_body


FORK_OWNER = "aionic-labs"
REPO_NAME = "sapling"
FORK_REPO_ID = "R_aionic_labs_sapling"


def setup_mock_github_server() -> MockGitHubServer:
    github_server = MockGitHubServer()
    parent = {
        "id": "R_facebook_sapling",
        "owner": {"login": "facebook"},
        "name": REPO_NAME,
        "isFork": False,
        "defaultBranchRef": {"name": "main"},
        "parent": None,
    }
    github_server.expect_get_repository_request(
        owner=FORK_OWNER, name=REPO_NAME
    ).and_respond(
        repo_id=FORK_REPO_ID,
        is_fork=True,
        parent=parent,
    )
    github_server.expect_guess_next_pull_request_number(
        owner=FORK_OWNER, name=REPO_NAME
    ).and_respond()

    number = 42
    message = "one\n"
    title, body = title_and_body(message)
    github_server.expect_create_pr_request(
        owner=FORK_OWNER,
        name=REPO_NAME,
        body=body,
        title=title,
        head=f"pr{number}",
        base="main",
        is_draft=True,
    ).and_respond(number=number)

    pr_id = f"PR_id_{number}"
    github_server.expect_get_pr_details_request(
        number, owner=FORK_OWNER, name=REPO_NAME
    ).and_respond(pr_id, is_draft=True)
    github_server.expect_update_pr_request(
        pr_id,
        number,
        message,
        owner=FORK_OWNER,
        name=REPO_NAME,
        review_url="unused-for-a-single-pr",
        review_tool="ReviewStack",
    ).and_respond()
    github_server.expect_request_reviewers(
        number, "alice", owner=FORK_OWNER, name=REPO_NAME
    )

    github_server.expect_get_username_request().and_respond()
    github_server.expect_merge_into_branch(
        "ebe5b8faff36687becb7bdbca1e6a61dac428834",
        repo_id=FORK_REPO_ID,
    ).and_respond()
    return github_server


def uisetup(ui):
    mock_github_server = setup_mock_github_server()
    extensions.wrapfunction(
        github_gh_cli, "_make_request", mock_github_server.make_request
    )
    extensions.wrapfunction(submit, "run_git_command", mock_run_git_command)
