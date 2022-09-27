# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import re
from typing import Optional

from edenscm import error, scmutil
from edenscm.i18n import _

from .pullrequeststore import PullRequest, PullRequestStore


def link(ui, repo, *args, **opts):
    if len(args) != 1:
        raise error.Abort(_("must specify a pull request"))

    pr_arg = args[0]
    pull_request = resolve_pr_arg(pr_arg, ui)
    if not pull_request:
        raise error.Abort(_("could not resolve pull request: '%%s'") % pr_arg)

    ctx = scmutil.revsingle(repo, opts.get("rev"), None)
    pr_store = PullRequestStore(repo)
    pr_store.map_commit_to_pull_request(ctx.node(), pull_request)


def unlink(ui, repo, *args, **opts):
    ctx = scmutil.revsingle(repo, opts.get("rev"), None)
    pr_store = PullRequestStore(repo)
    pr_store.unlink(ctx.node())


def resolve_pr_arg(pr_arg: str, ui) -> Optional[PullRequest]:
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


def try_parse_pull_request_url(url: str) -> Optional[PullRequest]:
    """parses the url into a PullRequest if it is in the expected format"""
    pattern = r"^https://github.com/([^/]+)/([^/]+)/pull/([1-9][0-9]+)$"
    match = re.match(pattern, url)
    if match:
        pull_request = PullRequest()
        pull_request.owner = match[1]
        pull_request.name = match[2]
        pull_request.number = int(match[3])
        return pull_request
    else:
        return None


def try_find_upstream(ui) -> Optional[str]:
    """checks [paths] in .hgrc for an upstream GitHub repo"""
    for remote in ["upstream", "default"]:
        url = ui.config("paths", "upstream")
        if url:
            repo_url = normalize_github_repo_url(url)
            if repo_url:
                return repo_url

    return None


def normalize_github_repo_url(url: str) -> Optional[str]:
    """parses the following URL formats:

    https://github.com/bolinfest/escoria-demo-game
    https://github.com/bolinfest/escoria-demo-game.git
    git@github.com:bolinfest/escoria-demo-game.git

    and returns:

    https://github.com/bolinfest/escoria-demo-game

    which is suitable for constructing URLs to pull requests.
    """
    https_pattern = r"^https://github.com/([^/]+)/([^/]+?)(?:\.git)?$"
    https_match = re.match(https_pattern, url)
    if https_match:
        return f"https://github.com/{https_match[1]}/{https_match[2]}"

    ssh_pattern = r"^git@github.com:([^/]+)/([^/]+?)(?:\.git)?$"
    ssh_match = re.match(ssh_pattern, url)
    if ssh_match:
        return f"https://github.com/{ssh_match[1]}/{ssh_match[2]}"

    return None
