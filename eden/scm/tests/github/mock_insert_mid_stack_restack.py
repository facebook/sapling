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

# The --restack counterpart of mock_insert_mid_stack.py: the diverged stack
# (#42, #43, #44 with a new commit inserted between #42 and #43) is dissolved
# up front, the base branches are re-chained (including through the new pull
# request #45), everything is pushed, and the stack is recreated from the
# local stack.


def setup_mock_github_server(repo) -> MockGitHubServer:
    github_server = MockGitHubServer()

    github_server.expect_get_repository_request().and_respond()

    commit_one = scmutil.revsingle(repo, "desc(one)").hex()
    github_server.expect_get_pr_details_request(42).and_respond(
        "PR_id_42", head_ref_oid=commit_one
    )
    github_server.expect_get_pr_details_request(43).and_respond("PR_id_43")
    github_server.expect_get_pr_details_request(44).and_respond("PR_id_44")

    github_server.expect_guess_next_pull_request_number().and_respond(
        latest_issue_num=40, latest_pr_num=44
    )

    github_server.expect_get_stack_request(42).and_respond(
        stack_number=100, pr_numbers=[42, 43, 44]
    )

    # The diverged stack is dissolved before any base updates.
    github_server.expect_unstack_request(100).and_respond()

    # Base branches are re-chained through the new pull request: #43's base
    # becomes the inserted commit's head branch (pr45).
    github_server.expect_update_pr_request(
        "PR_id_44", 44, "", base="pr43"
    ).and_respond()
    github_server.expect_update_pr_request(
        "PR_id_43", 43, "", base="pr45"
    ).and_respond()
    github_server.expect_update_pr_request(
        "PR_id_42", 42, "", base="main"
    ).and_respond()

    # The inserted commit's pull request is created directly against the head
    # branch of the pull request below it.
    github_server.expect_create_pr_request(
        body="", title="insert", head="pr45", base="pr42"
    ).and_respond(number=45)
    github_server.expect_get_pr_details_request(45).and_respond("PR_id_45")

    # Body rewrites leave the base branch untouched in the stacked workflow.
    github_server.expect_update_pr_request(
        "PR_id_44", 44, "three\n", base=None
    ).and_respond()
    github_server.expect_update_pr_request(
        "PR_id_43", 43, "two\n", base=None
    ).and_respond()
    github_server.expect_update_pr_request(
        "PR_id_45", 45, "insert\n", base=None
    ).and_respond()
    github_server.expect_update_pr_request(
        "PR_id_42", 42, "one\n", base=None
    ).and_respond()

    github_server.expect_get_username_request().and_respond()
    tip = scmutil.revsingle(repo, "desc(three)").hex()
    github_server.expect_merge_into_branch(tip).and_respond()

    # The stack is recreated to match the local stack, including the inserted
    # pull request.
    github_server.expect_create_stack_request([42, 45, 43, 44]).and_respond(
        stack_number=101
    )

    return github_server


def reposetup(ui, repo):
    github_server = setup_mock_github_server(repo)
    extensions.wrapfunction(
        github_gh_cli, "_make_request", github_server.make_request
    )
    extensions.wrapfunction(submit, "run_git_command", mock_run_git_command)
    wrap_with_consumption_check(github_server, submit, "submit")
