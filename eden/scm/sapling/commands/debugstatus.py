# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from ..i18n import _, _x
from .cmdtable import command


@command("debugstatus", [("n", "nonnormal", 0, _("print nonnormalfiltered samples"))])
def debugstatus(ui, repo, **opts):
    """common performance issues for status"""
    dirstate = repo.dirstate
    dmap = dirstate._map
    dmap_len = None
    try:
        dmap_len = str(len(dmap))
    except RuntimeError:
        dmap_len = "not supproted"
    ui.write(_x("len(dirstate) = %s\n") % (dmap_len,))

    nonnormalset = dmap.nonnormalset
    ui.write(_x("len(nonnormal) = %d\n") % len(nonnormalset))

    visitdir = dirstate._ignore.visitdir

    def dirfilter(path):
        return visitdir(path) == "all"

    nonnormalfiltered = dmap.nonnormalsetfiltered(dirfilter)
    ui.write(_x("len(filtered nonnormal) = %d\n") % len(nonnormalfiltered))

    toprint = int(opts.get("nonnormal", 0))
    if toprint:
        for path in sorted(nonnormalfiltered)[:toprint]:
            ui.write(_x("  %s\n") % path)

    ui.write(_x("clock = %s\n") % dirstate.getclock())
