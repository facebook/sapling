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
from sapling.ext.github.pull_request_body import title_and_body

# An extension to mock network requests for `sl pr submit` with
# github.pr-workflow=stacked when the repo is a fork. GitHub requires all
# branches of a stack to be in the same repository, so no native stack is
# created (a warning is printed instead), and chained bases are not possible
# (fork head branches cannot be bases on the upstream repository), so both
# pull requests are created against the upstream default branch.

UPSTREAM = {
    "id": "R_upstream_repo",
    "owner": {"id": "upstream_id", "login": "upstream"},
    "name": "test_github_repo",
    "isFork": False,
    "defaultBranchRef": {"name": "main"},
}


def setup_mock_github_server(repo) -> MockGitHubServer:
    github_server = MockGitHubServer()

    github_server.expect_get_repository_request().and_respond(
        is_fork=True, parent=UPSTREAM
    )

    github_server.expect_guess_next_pull_request_number(
        owner="upstream"
    ).and_respond()

    prs = [
        (42, "one\n"),
        (43, "two\n"),
    ]
    for num, msg in prs:
        title, body = title_and_body(msg)
        github_server.expect_create_pr_request(
            body=body,
            title=title,
            head=f"facebook:pr{num}",
            base="main",
            owner="upstream",
        ).and_respond(number=num)

        pr_id = f"PR_id_{num}"
        github_server.expect_get_pr_details_request(
            num, owner="upstream"
        ).and_respond(pr_id)

        # The stacked workflow leaves the base untouched when rewriting the
        # body, and omits the stack list footer.
        github_server.expect_update_pr_request(
            pr_id, num, msg, base=None, owner="upstream"
        ).and_respond()

    github_server.expect_get_username_request().and_respond()
    tip = scmutil.revsingle(repo, "desc(two)").hex()
    github_server.expect_merge_into_branch(tip).and_respond()

    return github_server


def reposetup(ui, repo):
    github_server = setup_mock_github_server(repo)
    extensions.wrapfunction(
        github_gh_cli, "_make_request", github_server.make_request
    )
    extensions.wrapfunction(submit, "run_git_command", mock_run_git_command)
    wrap_with_consumption_check(github_server, submit, "submit")
