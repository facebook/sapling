# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from sapling import extensions
from sapling.ext.github import github_gh_cli, import_pull_request
from sapling.ext.github.mock_utils import (
    MockGitHubServer,
    wrap_with_consumption_check,
)

# An extension to mock network requests for `sl pr pull` of the pull request
# at the bottom of a native GitHub stack: there are no ancestors to link, so
# no warning is printed and no ancestor details are fetched.

COMMIT_ONE = "ebe5b8faff36687becb7bdbca1e6a61dac428834"


def setup_mock_github_server(ui) -> MockGitHubServer:
    github_server = MockGitHubServer()

    github_server.expect_get_pr_details_request(42).and_respond(
        "PR_id_42", head_ref_oid=COMMIT_ONE
    )

    # #42 is the bottom of native stack #100.
    github_server.expect_get_stack_request(42).and_respond(
        stack_number=100, pr_numbers=[42, 43, 44]
    )

    return github_server


def uisetup(ui):
    github_server = setup_mock_github_server(ui)
    extensions.wrapfunction(
        github_gh_cli, "_make_request", github_server.make_request
    )
    wrap_with_consumption_check(github_server, import_pull_request, "get_pr")
