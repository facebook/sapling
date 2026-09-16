# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from sapling import extensions
from sapling.ext.github import github_gh_cli, submit
from sapling.ext.github.consts import GITHUB_HOSTNAME
from sapling.ext.github.mock_utils import (
    mock_run_git_command,
    MockGitHubServer,
    OWNER,
    REPO_NAME,
)
from sapling.ext.github.pull_request_body import (
    _format_review_url,
    DEFAULT_REVIEW_TOOL_NAME,
    DEFAULT_REVIEW_URL_TEMPLATE,
    title_and_body,
)


def setup_mock_github_server() -> MockGitHubServer:
    github_server = MockGitHubServer()
    github_server.expect_get_repository_request().and_respond()
    github_server.expect_guess_next_pull_request_number().and_respond()

    number = 42
    message = "two\n"
    title, body = title_and_body(message)
    github_server.expect_create_pr_request(
        body=body,
        title=title,
        head=f"pr{number}",
        base="main",
        is_draft=True,
    ).and_respond(number=number)

    pr_id = f"PR_id_{number}"
    github_server.expect_get_pr_details_request(number).and_respond(pr_id, is_draft=True)
    github_server.expect_update_pr_request(
        pr_id,
        number,
        message,
        review_url=_format_review_url(
            DEFAULT_REVIEW_URL_TEMPLATE,
            owner=OWNER,
            repo=REPO_NAME,
            number=number,
            hostname=GITHUB_HOSTNAME,
        ),
        review_tool=DEFAULT_REVIEW_TOOL_NAME,
    ).and_respond()
    github_server.expect_request_reviewers(number, "alice")
    github_server.expect_request_reviewers(number, "bob")

    github_server.expect_get_username_request().and_respond()
    github_server.expect_merge_into_branch(
        "1a67244b0a776bfcc3be6bf811e98c993d78ce47"
    ).and_respond()
    return github_server


def uisetup(ui):
    mock_github_server = setup_mock_github_server()
    extensions.wrapfunction(
        github_gh_cli, "_make_request", mock_github_server.make_request
    )
    extensions.wrapfunction(submit, "run_git_command", mock_run_git_command)
