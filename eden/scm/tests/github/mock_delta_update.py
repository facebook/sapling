# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os

from sapling import error, extensions
from sapling.ext.github import github_gh_cli, submit
from sapling.ext.github.consts import query
from sapling.ext.github.mock_utils import (
    mock_run_git_command,
    OWNER,
    OWNER_ID,
    REPO_ID,
    REPO_NAME,
    USER_NAME,
)
from sapling.result import Ok


ARCHIVED_DELTA_HEAD = False
PUSHED = False


def _env(name: str) -> str:
    value = os.environ.get(name)
    if not value:
        raise error.Abort(f"missing {name}")
    return value


async def _make_request(
    real_make_request,
    params,
    hostname,
    endpoint="graphql",
    method=None,
):
    request_query = params.get("query")
    if request_query == query.GRAPHQL_GET_REPOSITORY:
        return Ok(
            {
                "data": {
                    "repository": {
                        "id": REPO_ID,
                        "owner": {"id": OWNER_ID, "login": OWNER},
                        "name": REPO_NAME,
                        "isFork": False,
                        "defaultBranchRef": {"name": "main"},
                        "parent": None,
                    }
                }
            }
        )

    if request_query == query.GRAPHQL_GET_PULL_REQUEST:
        number = params["number"]
        head_oid = _env(f"SL_TEST_PR{number}_HEAD")
        title = "one" if number == 42 else "two"
        return Ok(
            {
                "data": {
                    "repository": {
                        "pullRequest": {
                            "id": f"PR_id_{number}",
                            "url": f"https://github.com/{OWNER}/{REPO_NAME}/pull/{number}",
                            "state": "OPEN",
                            "headRefOid": head_oid,
                            "headRefName": f"pr{number}",
                            "baseRefOid": "base",
                            "baseRefName": "main",
                            "body": "",
                            "title": title,
                        }
                    }
                }
            }
        )

    if request_query == query.GRAPHQL_UPDATE_PULL_REQUEST:
        return Ok(
            {
                "data": {
                    "updatePullRequest": {
                        "pullRequest": {"id": params["pullRequestId"]}
                    }
                }
            }
        )

    if request_query == query.GRAPHQL_GET_LOGIN:
        return Ok({"data": {"viewer": {"login": USER_NAME}}})

    if request_query == query.GRAPHQL_MERGE_BRANCH:
        global ARCHIVED_DELTA_HEAD
        old_head = _env("SL_TEST_DELTA_OLD")
        new_head = _env("SL_TEST_DELTA_NEW")
        if params["head"] == old_head:
            if PUSHED:
                raise error.Abort("old delta head archived after push")
            ARCHIVED_DELTA_HEAD = True
        elif params["head"] == new_head:
            if not PUSHED:
                raise error.Abort("new head archived before push")
        else:
            raise error.Abort(f"unexpected archive head: {params['head']}")
        return Ok(
            {
                "data": {
                    "mergeBranch": {
                        "mergeCommit": {"oid": f"merge-{params['head'][:12]}"}
                    }
                }
            }
        )

    if request_query == query.GRAPHQL_ADD_COMMENT:
        expected_pr = _env("SL_TEST_DELTA_PR")
        old_head = _env("SL_TEST_DELTA_OLD")
        new_head = _env("SL_TEST_DELTA_NEW")
        if not ARCHIVED_DELTA_HEAD:
            raise error.Abort("delta comment posted before old head was archived")
        if not PUSHED:
            raise error.Abort("delta comment posted before push")
        expected_subject = f"PR_id_{expected_pr}"
        if params["subjectId"] != expected_subject:
            raise error.Abort(
                f"expected delta comment on {expected_subject}, got {params['subjectId']}"
            )
        body = params["body"]
        expected_compare = (
            f"https://github.com/{OWNER}/{REPO_NAME}/compare/{old_head}..{new_head}"
        )
        for expected in [
            f"sapling-pr-delta old={old_head} new={new_head}",
            expected_compare,
        ]:
            if expected not in body:
                raise error.Abort(f"missing {expected!r} from {body!r}")
        return Ok(
            {
                "data": {
                    "addComment": {"commentEdge": {"node": {"id": "delta_comment"}}}
                }
            }
        )

    raise error.Abort(f"unexpected GitHub request: {params}")


def _run_git_command(real_run_git_command, args, gitdir):
    global PUSHED
    if args and args[0] == "push":
        if not ARCHIVED_DELTA_HEAD:
            raise error.Abort("pushed before old delta head was archived")
        PUSHED = True
    return mock_run_git_command(real_run_git_command, args, gitdir)


def uisetup(ui):
    extensions.wrapfunction(github_gh_cli, "_make_request", _make_request)
    extensions.wrapfunction(submit, "run_git_command", _run_git_command)
