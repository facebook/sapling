# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from sapling import extensions
from sapling.ext.github import github_gh_cli, submit
from sapling.ext.github.mock_utils import mock_run_git_command, MockGitHubServer
from sapling.ext.github.pull_request_body import title_and_body

# An extension to mock network requests for the initial `sl pr submit` of a
# stack of two commits with github.pr-workflow=stacked. It replaces
# `github_gh_cli.make_request` and `submit.run_git_command` with the
# corresponding mock functions. Check the `uisetup` function for how the mock
# functions are registered.


def setup_mock_github_server(ui) -> MockGitHubServer:
    """Setup mock GitHub Server for testing happy case of `sl pr submit` with
    the "stacked" workflow.
    """
    github_server = MockGitHubServer()

    github_server.expect_get_repository_request().and_respond()

    github_server.expect_guess_next_pull_request_number().and_respond()

    prs = [
        (42, "one\n"),
        (43, "two\n"),
    ]

    for idx, (num, msg) in enumerate(prs):
        title, body = title_and_body(msg)
        head = f"pr{num}"

        # Each PR's base is chained to the head branch of the PR below it.
        base = "main" if idx == 0 else "pr%d" % prs[idx - 1][0]

        github_server.expect_create_pr_request(
            body=body,
            title=title,
            head=head,
            base=base,
        ).and_respond(number=num)

        pr_id = f"PR_id_{num}"
        github_server.expect_get_pr_details_request(num).and_respond(pr_id)

        # The "stacked" workflow omits the stack list footer from PR bodies
        # (GitHub renders the stack natively), so stack_pr_ids is left unset.
        # It also leaves the base branch untouched when rewriting the body
        # (base=None), since the native stack manages base branches.
        github_server.expect_update_pr_request(
            pr_id, num, msg, base=None
        ).and_respond()

    github_server.expect_get_username_request().and_respond()

    head = "1a67244b0a776bfcc3be6bf811e98c993d78ce47"
    github_server.expect_merge_into_branch(head).and_respond()

    # Neither the bottom (#42) nor the top (#43) pull request is part of a
    # stack yet, so a new stack is created.
    github_server.expect_get_stack_request(42).and_respond()
    github_server.expect_get_stack_request(43).and_respond()
    github_server.expect_create_stack_request([42, 43]).and_respond(stack_number=100)

    return github_server


def uisetup(ui):
    mock_github_server = setup_mock_github_server(ui)
    extensions.wrapfunction(
        github_gh_cli, "_make_request", mock_github_server.make_request
    )
    extensions.wrapfunction(submit, "run_git_command", mock_run_git_command)
