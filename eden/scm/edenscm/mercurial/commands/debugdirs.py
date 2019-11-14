# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

from .. import scmutil, util
from ..i18n import _
from . import debug
from .cmdtable import command


@command(
    b"debugdirs",
    [
        ("r", "rev", "", _("search the repository as it is in REV"), _("REV")),
        ("0", "print0", None, _("end filenames with NUL, for use with xargs")),
    ],
    _("[DIR]..."),
    cmdtype=command.readonly,
)
def debugdirs(ui, repo, *dirs, **opts):
    """list directories

    This is analogous to using ``hg files`` to list which files exist, except
    for directories.
    """
    ctx = scmutil.revsingle(repo, opts.get("rev"), None)
    end = "\n"
    if opts.get("print0"):
        end = "\0"
    fmt = "%s" + end

    treemanifest = debug._findtreemanifest(ctx)
    if treemanifest:
        for d in dirs:
            if treemanifest.hasdir(d.strip("/")):
                ui.write(fmt % d)
    else:
        candidates = {d.strip("/"): d for d in dirs}
        matches = set()
        for f in ctx.manifest().iterkeys():
            for p in util.finddirs(f):
                if p in candidates:
                    matches.add(candidates.pop(p))
            if not candidates:
                break
        for d in dirs:
            if d in matches:
                ui.write(fmt % d)
