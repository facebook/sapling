# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""make commits without a working copy

With this extension enabled, Mercurial provides a command i.e. `memcommit` to
make commits to a repository without requiring a working copy.

TODO: add the `memcommit` command.
"""

from __future__ import absolute_import

from edenscm.mercurial import registrar, scmutil
from edenscm.mercurial.i18n import _

from . import commitdata


cmdtable = {}
command = registrar.command(cmdtable)


@command(
    "^debugserializecommit",
    [
        ("r", "rev", "", _("revision to serialize"), _("REV")),
        ("d", "dest", "", _("destination bookmark"), _("DEST")),
        ("", "pushrebase", False, _("pushrebase commit")),
    ],
    _("hg debugserializecommit -r REV -d DEST"),
)
def debugserializecommit(ui, repo, *args, **opts):
    """serialize commit in format consumable by 'memcommit' command

    If no revision for serialization is specified, the current commit is
    serialized.

    This command is mainly intended for the testing the command for making
    commits.
    """
    ctx = scmutil.revsingle(repo, opts.get("rev"))
    changelistbuilder = commitdata.changelistbuilder(ctx.p1().hex())

    for path in ctx.files():
        if path in ctx:
            fctx = ctx[path]
            renamed = fctx.renamed()
            copysource = renamed[0] if renamed else None

            info = commitdata.fileinfo(
                flags=fctx.flags(), content=fctx.data(), copysource=copysource
            )
        else:
            info = commitdata.fileinfo(deleted=True)

        changelistbuilder.addfile(path, info)

    changelist = changelistbuilder.build()
    destination = commitdata.destination(
        bookmark=opts.get("dest"), pushrebase=opts.get("pushrebase")
    )

    metadata = commitdata.metadata(
        author=ctx.user(),
        description=ctx.description(),
        parents=[p.hex() for p in ctx.parents()],
        extra=ctx.extra(),
    )

    params = commitdata.params(changelist, metadata, destination)
    ui.write(params.serialize())
