# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import tempfile

import bindings

from . import progress
from .i18n import _
from .node import short


def checklazychangelog(repo):
    """check lazy changelog properties and print found problems

    This function only performs quick local checks to find some
    serious issues. It does not verify the local graph with the
    server.

    Return 127 if problems are found, 0 otherwise.
    """
    if "lazychangelog" not in repo.storerequirements:
        return 0

    ui = repo.ui
    commits: "bindings.dag.commits" = repo.changelog.inner
    problems = []
    missingids = commits.checkuniversalids()
    if missingids:
        problems.append(
            _(
                "part of the commit graph cannot resolve commit "
                "hashes because missing commit hashes for ids: %r\n"
            )
            % (missingids,)
        )
    problems += commits.checksegments()
    if problems:
        ui.write(_("local commit graph has problems:\n"))
        for problem in problems:
            ui.write(_(" %s") % problem)
        ui.write(_("consider '%prog% debugrebuildchangelog' to recover\n"))
        return 127
    else:
        ui.status(_("commit graph passed quick local checks\n"))
    return 0


def checklazychangelogwithserver(repo):
    """check lazy changelog shape with the server and print found problems

    This check first performs a graph clone using segmented changelog protocols.
    Return 127 if problems are found, 0 otherwise.
    """
    if "lazychangelog" not in repo.storerequirements:
        return 0
    ui = repo.ui
    with progress.spinner(ui, _("getting initial clone data")):
        data = repo.edenapi.clonedata()
    with tempfile.TemporaryDirectory(prefix="hg-check-cl") as tmpdir:
        with progress.spinner(ui, _("importing clone data")):
            commits = bindings.dag.commits.openhybrid(
                None,  # no revlog
                os.path.join(tmpdir, "segments"),  # segments dir
                os.path.join(tmpdir, "hgcommits"),  # hgcommits dir
                repo.edenapi,
                lazyhash=True,
            )
            commits.importclonedata(data)
        dag = repo.changelog.dag
        heads = list(dag.heads(dag.mastergroup()))
        with progress.spinner(
            ui, _("comparing ancestors(%s)") % ("+".join([short(n) for n in heads]))
        ):
            problems = repo.changelog.inner.checkisomorphicgraph(commits, heads)
    if problems:
        ui.write(_("commit graph differs from the server:\n"))
        for problem in problems:
            ui.write(_(" %s") % problem)
        return 127
    else:
        ui.status(_("commit graph looks okay compared with the server\n"))
    return 0
