# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import time

from .. import merge as mergemod, progress, util
from ..i18n import _
from .cmdtable import command


@command("debugdryup", [], _("REV_FROM REV_TO"))
def debugdryup(ui, repo, fromspec, tospec, **opts):
    """Execute native checkout (update) without actually writing to working copy"""
    fromctx = repo[fromspec]
    toctx = repo[tospec]

    with PrintTimer(ui, "Calculating"), progress.spinner(ui, _("calculating")):
        plan = mergemod.makenativecheckoutplan(repo, fromctx, toctx)

    repo.ui.write(_("plan has %s actions\n") % len(plan))

    with PrintTimer(ui, "Fetching"):
        if repo.ui.configbool("nativecheckout", "usescmstore"):
            count, size = plan.apply_dry_run(
                repo.fileslog.filescmstore,
            )
        else:
            count, size = plan.apply_dry_run(
                repo.fileslog.contentstore,
            )

    repo.ui.write(_("fetched %s files with %s\n") % (count, util.bytecount(size)))


class PrintTimer(object):
    def __init__(self, ui, name):
        self.ui = ui
        self.name = name

    def __enter__(self):
        self.start = time.time()

    def __exit__(self, exc_type, exc_val, exc_tb):
        if exc_type is None:
            durations = time.time() - self.start
            self.ui.write("%s %s\n" % (self.name, util.formatduration(durations)))
