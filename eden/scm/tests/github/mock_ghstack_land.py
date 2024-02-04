# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import json
import subprocess

import ghstack
from ghstack import ghs_types, github_gh_cli, github_utils
from sapling import extensions
from sapling.ext.github.mock_utils import MockGitHubServer, OWNER, REPO_ID, REPO_NAME


def setup_mock_github_server() -> MockGitHubServer:
    github_server = MockGitHubServer()

    github_server.expect_request(
        params={
            "query": ghstack.query.GRAPHQL_GET_REPOSITORY,
            "owner": OWNER,
            "name": REPO_NAME,
        },
        response={
            "data": {
                "repository": {
                    "id": REPO_ID,
                    "isFork": False,
                    "defaultBranchRef": {"name": "main"},
                },
            },
        },
    )

    # Set up mocked responses for each draft commit in our stack.
    for (idx, draft_commit) in enumerate(
        subprocess.check_output(
            [
                "sl",
                "log",
                "-r",
                f"::. & draft()",
                "-T",
                "{dict(node,desc)|json}\n",
                # Disable ourself to prevent recursion.
                "--config",
                "extensions.mock_ghstack_land=!",
            ]
        ).splitlines()
    ):
        draft_commit = json.loads(draft_commit)

        commit_msg = draft_commit["desc"]
        commit_hash = draft_commit["node"]

        pr_number = idx + 1
        ghnum = idx

        body = """Stack from [ghstack](https://github.com/ezyang/ghstack) (oldest at bottom):
* (to be filled)

"""

        github_server.expect_create_pr_request(
            body=body,
            title=commit_msg,
            base=f"gh/test/{ghnum}/base",
            head=f"gh/test/{ghnum}/head",
            maintainer_can_modify=True,
            method="post",
        ).and_respond(pr_number)

        github_server.expect_pr_to_ref(
            pr_number=pr_number,
            ref=f"gh/test/{ghnum}/head",
        )

        github_server.expect_ref_to_commit_and_tree(
            ref=f"gh/test/{ghnum}/orig",
            commit=commit_hash,
            tree="not-used",
        )

    return github_server


ghnum = -1


def mock_get_next_ghnum(*args, **opts) -> ghs_types.GhNumber:
    global ghnum
    ghnum += 1
    return ghs_types.GhNumber(ghnum)


def mock_update_ref(*args, **opts) -> str:
    return "fake"


def mock_update_pr_body_and_title(*args, **opts):
    pass


def uisetup(ui):
    mock_github_server = setup_mock_github_server()
    extensions.wrapfunction(
        github_gh_cli, "_make_request", mock_github_server.make_request
    )

    # Mock these at Python level to make things simpler. Having
    # "realistic" interactions is not important for what we want to
    # test in the "land" flow.
    extensions.wrapfunction(github_utils, "get_next_ghnum", mock_get_next_ghnum)
    extensions.wrapfunction(github_utils, "update_ref", mock_update_ref)
    extensions.wrapfunction(
        ghstack.submit.Submitter,
        "_update_pr_body_and_title",
        mock_update_pr_body_and_title,
    )
