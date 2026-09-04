# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from sapling import extensions
from sapling.ext.github import github_gh_cli, import_pull_request
from sapling.ext.github.gh_submit import PullRequestState
from sapling.ext.github.mock_utils import (
    MockGitHubServer,
    wrap_with_consumption_check,
)

# An extension to mock network requests for `sl pr pull` of a pull request
# that is associated with a native GitHub stack but is no longer one of its
# open members (it was merged): its position within the stack is unknown, so
# ancestors cannot be linked, and a specific warning is printed (rather than
# the misleading "No stack information found in the pull request body").

COMMIT_THREE = "f4185fef85f10d46b859c30076243068b0f59245"


def setup_mock_github_server(ui) -> MockGitHubServer:
    github_server = MockGitHubServer()

    github_server.expect_get_pr_details_request(45).and_respond(
        "PR_id_45", head_ref_oid=COMMIT_THREE, state=PullRequestState.MERGED
    )

    # The stack query returns the stack, but #45 is not among its open
    # members (merged pull requests are excluded).
    github_server.expect_get_stack_request(45).and_respond(
        stack_number=100, pr_numbers=[42, 43]
    )

    return github_server


def uisetup(ui):
    github_server = setup_mock_github_server(ui)
    extensions.wrapfunction(
        github_gh_cli, "_make_request", github_server.make_request
    )
    wrap_with_consumption_check(github_server, import_pull_request, "get_pr")
