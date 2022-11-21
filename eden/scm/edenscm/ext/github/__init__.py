# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""utilities for interacting with GitHub (EXPERIMENTAL)
"""

from typing import Optional

from edenscm import commands, registrar
from edenscm.i18n import _

from . import follow, github_repo_util, link, pr_marker, pr_status, submit, templates

cmdtable = {}
command = registrar.command(cmdtable)
templatekeyword = registrar.templatekeyword()


def extsetup(ui):
    pr_status.setup_smartset_prefetch()


@command(
    "pr",
    [],
    _("SUBCOMMAND (default: submit)"),
)
def pull_request_command(ui, repo, *args, **opts):
    """exchange local commit data with GitHub pull requests"""
    return submit.submit(ui, repo, *args, **opts)


subcmd = pull_request_command.subcommand(
    categories=[
        ("Create or update pull requests", ["submit"]),
        (
            "Manually manage associations with pull requests",
            ["follow", "link", "unlink"],
        ),
    ]
)


@subcmd(
    "submit",
    [
        (
            "s",
            "stack",
            False,
            _("also include draft ancestors"),
        ),
        ("m", "message", None, _("message describing changes to updated commits")),
    ],
)
def submit_cmd(ui, repo, *args, **opts):
    """create or update GitHub pull requests from local commits"""
    return submit.submit(ui, repo, *args, **opts)


@subcmd(
    "link",
    [("r", "rev", "", _("revision to link"), _("REV"))],
    _("[-r REV] PULL_REQUEST"),
)
def link_cmd(ui, repo, *args, **opts):
    """indentify a commit as the head of a GitHub pull request

    A PULL_REQUEST can be specified in a number of formats:

    - GitHub URL to the PR: https://github.com/facebook/react/pull/42

    - Integer: Number for the PR. Uses 'paths.upstream' as the target repo,
        if specified; otherwise, falls back to 'paths.default'.
    """
    return link.link(ui, repo, *args, **opts)


@subcmd(
    "unlink",
    [
        ("r", "rev", [], _("revisions to unlink")),
    ],
    _("[OPTION]... [-r] REV..."),
)
def unlink_cmd(ui, repo, *revs, **opts):
    """remove a commit's association with a GitHub pull request"""
    revs = list(revs) + opts.pop("rev", [])
    return link.unlink(ui, repo, *revs)


@subcmd(
    "follow",
    [
        ("r", "rev", [], _("revisions to follow the next pull request")),
    ],
    _("[OPTION]... [-r] REV..."),
)
def follow_cmd(ui, repo, *revs, **opts):
    """join the nearest desecendant's pull request

    Marks commits to become part of their nearest desecendant's pull request
    instead of starting as the head of a new pull request.

    Use `pr unlink` to undo.
    """
    revs = list(revs) + opts.pop("rev", [])
    return follow.follow(ui, repo, *revs)


@command("debugprmarker", commands.dryrunopts)
def debug_pr_marker(ui, repo, **opts):
    dry_run = opts.get("dry_run")
    pr_marker.cleanup_landed_pr(repo, dry_run=dry_run)
    if dry_run:
        ui.status(_("(this is a dry-run, nothing was actually done)\n"))


@templatekeyword("github_repo")
def github_repo(repo, ctx, templ, **args) -> bool:
    try:
        return github_repo_util.check_github_repo(repo) is not None
    except Exception:
        return False


def _get_pull_request_field(field_name: str, repo, ctx, **args):
    pull_request_data = templates.get_pull_request_data_for_rev(repo, ctx, **args)
    return pull_request_data[field_name] if pull_request_data else None


@templatekeyword("github_pull_request_state")
def github_pull_request_state(repo, ctx, templ, **args) -> Optional[str]:
    return _get_pull_request_field("state", repo, ctx, **args)


@templatekeyword("github_pull_request_closed")
def github_pull_request_closed(repo, ctx, templ, **args) -> Optional[bool]:
    return _get_pull_request_field("closed", repo, ctx, **args)


@templatekeyword("github_pull_request_merged")
def github_pull_request_merged(repo, ctx, templ, **args) -> Optional[bool]:
    return _get_pull_request_field("merged", repo, ctx, **args)


@templatekeyword("github_pull_request_review_decision")
def github_pull_request_review_decision(repo, ctx, templ, **args) -> Optional[str]:
    return _get_pull_request_field("reviewDecision", repo, ctx, **args)


@templatekeyword("github_pull_request_is_draft")
def github_pull_request_is_draft(repo, ctx, templ, **args) -> Optional[bool]:
    return _get_pull_request_field("isDraft", repo, ctx, **args)


@templatekeyword("github_pull_request_title")
def github_pull_request_title(repo, ctx, templ, **args) -> Optional[str]:
    return _get_pull_request_field("title", repo, ctx, **args)


@templatekeyword("github_pull_request_body")
def github_pull_request_body(repo, ctx, templ, **args) -> Optional[str]:
    return _get_pull_request_field("body", repo, ctx, **args)


@templatekeyword("github_pull_request_url")
def github_pull_request_url(repo, ctx, templ, **args) -> Optional[str]:
    """If the commit is associated with a GitHub pull request, returns the URL
    for the pull request.
    """
    pull_request = templates.get_pull_request_url_for_rev(repo, ctx, **args)
    if pull_request:
        pull_request_domain = repo.ui.config("github", "pull_request_domain")
        return pull_request.as_url(domain=pull_request_domain)
    else:
        return None


@templatekeyword("github_pull_request_repo_owner")
def github_pull_request_repo_owner(repo, ctx, templ, **args) -> Optional[str]:
    """If the commit is associated with a GitHub pull request, returns the
    repository owner for the pull request.
    """
    return templates.github_pull_request_repo_owner(repo, ctx, **args)


@templatekeyword("github_pull_request_repo_name")
def github_pull_request_repo_name(repo, ctx, templ, **args) -> Optional[str]:
    """If the commit is associated with a GitHub pull request, returns the
    repository name for the pull request.
    """
    return templates.github_pull_request_repo_name(repo, ctx, **args)


@templatekeyword("github_pull_request_number")
def github_pull_request_number(repo, ctx, templ, **args) -> Optional[int]:
    """If the commit is associated with a GitHub pull request, returns the
    number for the pull request.
    """
    return templates.github_pull_request_number(repo, ctx, **args)


@templatekeyword("sapling_pr_follower")
def sapling_pr_follower(repo, ctx, templ, **args) -> bool:
    """Indicates if this commit is part of a pull request, but not the head commit."""
    store = templates.get_pull_request_store(repo, args["cache"])
    return store.is_follower(ctx.node())
