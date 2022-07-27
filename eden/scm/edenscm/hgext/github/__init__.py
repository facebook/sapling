# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""utilities for interacting with GitHub (EXPERIMENTAL)
"""

from typing import Optional

from edenscm.mercurial import registrar
from edenscm.mercurial.i18n import _

from . import github_repo as gh_repo, link, submit, templates

cmdtable = {}
command = registrar.command(cmdtable)
templatekeyword = registrar.templatekeyword()


@command(
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


@command(
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


@templatekeyword("github_repo")
def github_repo(repo, ctx, templ, **args) -> bool:
    return gh_repo.is_github_repo(repo)


@templatekeyword("github_pull_request_url")
def github_pull_request_url(repo, ctx, templ, **args) -> Optional[str]:
    """If the commit is associated with a GitHub pull request, returns the URL
    for the pull request.
    """
    descr = ctx.description()
    pull_request_domain = repo.ui.config("github", "pull_request_domain")
    return templates.github_pull_request_url(descr, pull_request_domain)


@templatekeyword("github_pull_request_repo_owner")
def github_pull_request_repo_owner(repo, ctx, templ, **args) -> Optional[str]:
    """If the commit is associated with a GitHub pull request, returns the
    repository owner for the pull request.
    """
    descr = ctx.description()
    return templates.github_pull_request_repo_owner(descr)


@templatekeyword("github_pull_request_repo_name")
def github_pull_request_repo_name(repo, ctx, templ, **args) -> Optional[str]:
    """If the commit is associated with a GitHub pull request, returns the
    repository name for the pull request.
    """
    descr = ctx.description()
    return templates.github_pull_request_repo_name(descr)


@templatekeyword("github_pull_request_number")
def github_pull_request_number(repo, ctx, templ, **args) -> Optional[int]:
    """If the commit is associated with a GitHub pull request, returns the
    number for the pull request.
    """
    descr = ctx.description()
    return templates.github_pull_request_number(descr)
