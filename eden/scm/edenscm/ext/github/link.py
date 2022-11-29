# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import re
from typing import Optional

from edenscm import error, scmutil
from edenscm.i18n import _

from . import github_repo_util
from .pullrequest import PullRequestId

from .pullrequeststore import PullRequestStore


def link(ui, repo, *args, **opts):
    if len(args) != 1:
        raise error.Abort(_("must specify a pull request"))

    pr_arg = args[0]
    pull_request = resolve_pr_arg(pr_arg, ui)
    if not pull_request:
        raise error.Abort(_("could not resolve pull request: '%%s'") % pr_arg)

    ctx = scmutil.revsingle(repo, opts.get("rev") or ".", None)
    pr_store = PullRequestStore(repo)
    pr_store.map_commit_to_pull_request(ctx.node(), pull_request)


def unlink(ui, repo, *revs):
    pr_store = PullRequestStore(repo)
    revs = set(scmutil.revrange(repo, revs))
    nodes = [repo[r].node() for r in revs]
    pr_store.unlink_all(nodes)


def resolve_pr_arg(pr_arg: str, ui) -> Optional[PullRequestId]:
    num = try_parse_int(pr_arg)
    if num:
        upstream = try_find_upstream(ui)
        if upstream:
            return try_parse_pull_request_url(f"{upstream}/pull/{num}")
        else:
            return None
    else:
        return try_parse_pull_request_url(pr_arg)


def try_parse_int(s: str) -> Optional[int]:
    """tries to parse s as a positive integer"""
    pattern = r"^[1-9][0-9]+$"
    match = re.match(pattern, s)
    return int(match[0]) if match else None


def try_parse_pull_request_url(url: str) -> Optional[PullRequestId]:
    """parses the url into a PullRequest if it is in the expected format"""
    pattern = r"^https://([^/]*)/([^/]+)/([^/]+)/pull/([1-9][0-9]+)$"
    match = re.match(pattern, url)
    if match:
        hostname, owner, name, number = match.groups()
        return PullRequestId(
            hostname=hostname, owner=owner, name=name, number=int(number)
        )
    else:
        return None


def try_find_upstream(ui) -> Optional[str]:
    """checks [paths] in .hgrc for an upstream GitHub repo"""
    for remote in ["upstream", "default"]:
        url = ui.config("paths", remote)
        if url:
            repo_url = normalize_github_repo_url(url)
            if repo_url:
                return repo_url

    return None


def normalize_github_repo_url(url: str) -> Optional[str]:
    repo = github_repo_util.parse_github_repo_from_github_url(url)
    return repo.to_url() if repo else None
