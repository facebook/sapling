# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from edenscm import extensions
from edenscm.ext.github import submit
from edenscm.ext.github.mock_utils import make_repository_for_response, mock_run_git_command, MockGitHubServer
from ghstack import github_gh_cli

# An extension to mock network requests by replacing the `github_gh_cli.make_request`
# and `submit.run_git_command` with the corresponding wrapper functions. Check `uisetup`
# function for how wrapper functions are registered.


def setup_mock_github_server() -> MockGitHubServer:
    """Setup mock GitHub Server for testing happy case of `sl pr submit` command."""
    github_server = MockGitHubServer()

    UPSTREAM_OWNER = "facebook"
    UPSTREAM_OWNER_ID = "U_facebook_id"
    UPSTREAM_REPO_NAME = "test_github_repo"
    UPSTREAM_REPO_ID = "R_facebook_test_github_repo"

    DOWNSTREAM_OWNER = "contributor"
    DOWNSTREAM_OWNER_ID = "U_contributor_id"
    DOWNSTREAM_REPO_NAME = "fork_github_repo"
    DOWNSTREAM_REPO_ID = "R_contributor_fork_github_repo"

    USER_NAME = "facebook_username"

    github_server.expect_get_repository_request(
        owner=DOWNSTREAM_OWNER,
        name=DOWNSTREAM_REPO_NAME,
    ).and_respond(
        repo_id=DOWNSTREAM_REPO_ID,
        owner_id=DOWNSTREAM_OWNER_ID,
        is_fork=True,
        parent=make_repository_for_response(
            repo_id=UPSTREAM_REPO_ID,
            repo_name=UPSTREAM_REPO_NAME,
            owner_id=UPSTREAM_OWNER_ID,
            owner_login=UPSTREAM_OWNER,
            is_fork=True,
        ),
    )

    pr_number = 2
    github_server.expect_create_pr_placeholder_request(
        owner=UPSTREAM_OWNER,
        name=UPSTREAM_REPO_NAME,
    ).and_respond(
        start_number=pr_number, num_times=1
    )

    body = "addfile\n"
    github_server.expect_create_pr_request(
        body=body,
        issue=pr_number,
        owner=UPSTREAM_OWNER,
        name=UPSTREAM_REPO_NAME,
        head=f"{DOWNSTREAM_OWNER}:pr{pr_number}",
    ).and_respond()

    pr_id = f"PR_id_{pr_number}"
    github_server.expect_get_pr_details_request(
        pr_number,
        owner=UPSTREAM_OWNER,
        name=UPSTREAM_REPO_NAME,
    ).and_respond(pr_id)

    github_server.expect_update_pr_request(
        pr_id,
        pr_number,
        body,
        owner=UPSTREAM_OWNER,
        name=UPSTREAM_REPO_NAME,
    ).and_respond()
    github_server.expect_get_username_request().and_respond()

    head = "3a120a3a153f7d2960967ce6f1d52698a4d3a436"
    github_server.expect_merge_into_branch(
        head,
        username=USER_NAME,
        repo_id=DOWNSTREAM_REPO_ID,
    ).and_respond()

    return github_server


def uisetup(ui):
    mock_github_server = setup_mock_github_server()
    extensions.wrapfunction(
        github_gh_cli, "make_request", mock_github_server.make_request
    )
    extensions.wrapfunction(submit, "run_git_command", mock_run_git_command)
