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

# An extension to mock network requests for the initial `sl pr submit` of a
# stack of two commits with github.pr-workflow=stacked and
# github.placeholder-strategy=true: pull request numbers are reserved via
# placeholder issues, the pull requests are created with chained bases, and
# they are linked into a native GitHub stack.


def setup_mock_github_server(repo) -> MockGitHubServer:
    github_server = MockGitHubServer()

    github_server.expect_get_repository_request().and_respond()

    github_server.expect_create_pr_placeholder_request().and_respond(
        start_number=42, num_times=2
    )

    prs = [
        (42, "one\n"),
        (43, "two\n"),
    ]
    for idx, (num, msg) in enumerate(prs):
        _title, body = title_and_body(msg)

        # Each PR's base is chained to the head branch of the PR below it.
        base = "main" if idx == 0 else "pr%d" % prs[idx - 1][0]
        github_server.expect_create_pr_using_placeholder_request(
            body=body, issue=num, base=base
        ).and_respond()

        pr_id = f"PR_id_{num}"
        github_server.expect_get_pr_details_request(num).and_respond(pr_id)

        # The stacked workflow leaves the base untouched when rewriting the
        # body, and omits the stack list footer.
        github_server.expect_update_pr_request(
            pr_id, num, msg, base=None
        ).and_respond()

    github_server.expect_get_username_request().and_respond()
    tip = scmutil.revsingle(repo, "desc(two)").hex()
    github_server.expect_merge_into_branch(tip).and_respond()

    # Neither pull request is part of a stack yet, so a new stack is created.
    github_server.expect_get_stack_request(42).and_respond()
    github_server.expect_get_stack_request(43).and_respond()
    github_server.expect_create_stack_request([42, 43]).and_respond(
        stack_number=100
    )

    return github_server


def reposetup(ui, repo):
    github_server = setup_mock_github_server(repo)
    extensions.wrapfunction(
        github_gh_cli, "_make_request", github_server.make_request
    )
    extensions.wrapfunction(submit, "run_git_command", mock_run_git_command)
    wrap_with_consumption_check(github_server, submit, "submit")
