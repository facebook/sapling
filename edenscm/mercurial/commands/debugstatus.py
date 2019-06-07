# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from .. import error
from ..i18n import _
from .cmdtable import command


@command("debugstatus", [("n", "nonnormal", 0, _("print nonnormalfiltered samples"))])
def debugstatus(ui, repo, **opts):
    """common performance issues for status"""
    if "treestate" not in repo.requirements:
        raise error.Abort("debugstatus only supports treestate currently")
    if "eden" in repo.requirements:
        raise error.Abort("debugstatus is not supported in edenfs virtual checkouts")

    dirstate = repo.dirstate
    dmap = dirstate._map
    ui.write(("len(dirstate) = %d\n") % len(dmap))

    nonnormalset = dmap.nonnormalset
    ui.write(("len(nonnormal) = %d\n") % len(nonnormalset))

    visitdir = dirstate._ignore.visitdir

    def dirfilter(path):
        return visitdir(path) == "all"

    nonnormalfiltered = dmap.nonnormalsetfiltered(dirfilter)
    ui.write(("len(filtered nonnormal) = %d\n") % len(nonnormalfiltered))

    toprint = int(opts.get("nonnormal", 0))
    if toprint:
        for path in sorted(nonnormalfiltered)[:toprint]:
            ui.write(("  %s\n") % path)

    ui.write(("clock = %s\n") % dirstate.getclock())
