# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""mark pull requests as "Landed" on pull

Currently, it only supports Github, we may extend it to other git hosting
providers (e.g. GitLab).
"""

from edenscm import commands, registrar
from edenscm.ext.github import pr_marker as github_pr_marker
from edenscm.i18n import _

cmdtable = {}
command = registrar.command(cmdtable)


@command("debugprmarker", commands.dryrunopts)
def debug_pr_marker(ui, repo, **opts):
    dry_run = opts.get("dry_run")
    github_pr_marker.cleanup_landed_pr(repo, dry_run=dry_run)
    if dry_run:
        ui.status(_("(this is a dry-run, nothing was actually done)\n"))
