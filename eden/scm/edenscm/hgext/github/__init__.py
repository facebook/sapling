# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""utilities for interacting with GitHub (EXPERIMENTAL)
"""

from edenscm.mercurial import registrar
from edenscm.mercurial.i18n import _

from . import link, submit

cmdtable = {}
command = registrar.command(cmdtable)


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
