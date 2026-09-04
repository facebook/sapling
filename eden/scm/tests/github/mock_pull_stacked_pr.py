# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from sapling import extensions
from sapling.ext.github import github_gh_cli
from sapling.ext.github.mock_utils import MockGitHubServer

# An extension to mock network requests for `sl pr pull` of a pull request
# that is part of a native GitHub stack: its body contains no Sapling stack
# list footer (github.pr-workflow=stacked omits it), so the ancestors must be
# discovered via the stacks API.

COMMIT_ONE = "ebe5b8faff36687becb7bdbca1e6a61dac428834"
COMMIT_TWO = "1a67244b0a776bfcc3be6bf811e98c993d78ce47"
COMMIT_THREE = "f4185fef85f10d46b859c30076243068b0f59245"


def setup_mock_github_server(ui) -> MockGitHubServer:
    github_server = MockGitHubServer()

    # Details for the pulled PR (#44). Its body is empty: no stack list
    # footer.
    github_server.expect_get_pr_details_request(44).and_respond(
        "PR_id_44", head_ref_oid=COMMIT_THREE
    )

    # #44 is the top of native stack #100, so #43 and #42 are its ancestors.
    github_server.expect_get_stack_request(44).and_respond(
        stack_number=100, pr_numbers=[42, 43, 44]
    )

    # The ancestors are linked by fetching their details.
    github_server.expect_get_pr_details_request(43).and_respond(
        "PR_id_43", head_ref_oid=COMMIT_TWO
    )
    github_server.expect_get_pr_details_request(42).and_respond(
        "PR_id_42", head_ref_oid=COMMIT_ONE
    )

    return github_server


def uisetup(ui):
    mock_github_server = setup_mock_github_server(ui)
    extensions.wrapfunction(
        github_gh_cli, "_make_request", mock_github_server.make_request
    )
