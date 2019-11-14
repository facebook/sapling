# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""make commits without a working copy

With this extension enabled, Mercurial provides a command i.e. `memcommit` to
make commits to a repository without requiring a working copy.

Config::

    [memcommit]
    # allow creating commits with no parents.
    allowunrelatedroots = False
"""

from __future__ import absolute_import

import contextlib
import json
import sys

from edenscm.mercurial import bookmarks, error, registrar, scmutil
from edenscm.mercurial.i18n import _
from edenscm.mercurial.node import hex, nullid

from ..pushrebase.stackpush import pushrequest
from . import commitdata, serialization


configtable = {}
configitem = registrar.configitem(configtable)
configitem("memcommit", "allowunrelatedroots", default=False)

cmdtable = {}
command = registrar.command(cmdtable)


@command(
    "debugserializecommit",
    [
        ("r", "rev", "", _("revision to serialize"), _("REV")),
        ("d", "dest", "", _("destination bookmark"), _("DEST")),
        ("", "to", "", _("destination parents"), _("TO")),
        ("", "pushrebase", False, _("pushrebase commit")),
    ],
    _("hg debugserializecommit -r REV -d DEST"),
)
def debugserializecommit(ui, repo, *args, **opts):
    """serialize commit in format consumable by 'memcommit' command

    If no revision for serialization is specified, the current commit is
    serialized.

    This command is mainly intended for the testing the 'memcommit' command.
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

    bookmark = opts.get("dest")
    to = opts.get("to")
    pushrebase = opts.get("pushrebase")

    destination = commitdata.destination(bookmark=bookmark, pushrebase=pushrebase)

    parents = (
        [hex(p) for p in repo.nodes(to)] if to else [p.hex() for p in ctx.parents()]
    )

    metadata = commitdata.metadata(
        author=ctx.user(),
        description=ctx.description(),
        parents=parents,
        extra=ctx.extra(),
    )

    params = commitdata.params(changelist, metadata, destination)
    ui.write(serialization.serialize(params.todict()))


@command("memcommit", [], _("hg memcommit"))
def memcommit(ui, repo, *args, **opts):
    """make commits without a working copy

    This command supports creating commits in three different ways::

        - Commit on a specified parent

          In this case, we will create a commit on top of the specified parent.

        - Commit on a specified bookmark

          In this case, we will create a commit on top of the specified
          bookmark. For now, we require that the specified bookmark refers to
          the same commit as the specified parent.

          After creating the commit, we move the bookmark to refer to the new
          commit.

        - Commit on a specified bookmark using pushrebase

          In this case, we will create a commit only if the parent commit is an
          ancestor or descendant of the specified bookmark.

            - Case I: commit parent is an ancestor of bookmark

               o bookmark
               :
               :
               :   o x
               :  /
               : /
               o

               We should pushrebase x onto bookmark as long as there are no
               merge conflicts.

             - Case II: commit parent is a descendant of bookmark

               o x
               :
               :
               :
               :
               :
               o bookmark

               We will only commit x if the repository already has parent of x.

          After creating the commit, we move the bookmark to refer to the new
          commit.

    The output of the command will be JSON based. There are two cases::

        - Commit was created successfully

          In this case, exit code will be zero and the output will be:

            { "hash": "<commithash>" }

          where '<commithash>' is the commit hash for the newly created commit.

        - Commit creation failed

          In this case, exit code will be non-zero and the output will be:

            { "error": "<error>" }

          where '<error>' will describe the error that occurred while attempting
          to make the commit.

    There will be no output if the `-q` i.e. quiet flag is specified.
    """

    @contextlib.contextmanager
    def nooutput(ui):
        ui.pushbuffer(error=True, subproc=True)
        try:
            yield
        finally:
            ui.popbuffer()

    out = {}
    try:
        with nooutput(ui):
            params = commitdata.params.fromdict(serialization.deserialize(ui.fin))
            out["hash"] = hex(_memcommit(repo, params))
    except Exception as ex:
        out["error"] = str(ex)
        sys.exit(255)
    finally:
        if not ui.quiet:
            ui.write(json.dumps(out))


def _memcommit(repo, params):
    """create a new commit in the repo based on the params

    Isolating this method allows easy wrapping by other extensions like hgsql.
    """

    with repo.wlock(), repo.lock(), repo.transaction("memcommit"):

        def resolvetargetctx(repo, originalparentnode, targetparents):
            numparents = len(targetparents)

            if numparents > 1:
                raise error.Abort(_("merge commits are not supported"))

            if numparents == 0:
                raise error.Abort(_("parent commit must be specified"))

            targetctx = repo[targetparents[0]]
            targetnode = targetctx.node()

            if originalparentnode != targetnode:
                raise error.Abort(_("commit with new parents not supported"))

            if (
                not repo.ui.configbool("memcommit", "allowunrelatedroots")
                and targetnode == nullid
            ):
                raise error.Abort(_("commit without parents are not allowed"))

            return targetctx

        request = pushrequest.frommemcommit(repo, params)
        originalparentnode = request.stackparentnode
        targetctx = resolvetargetctx(repo, originalparentnode, params.metadata.parents)
        targetnode = targetctx.node()

        destination = params.destination
        pushrebase = destination.pushrebase
        ontobookmark = destination.bookmark

        if ontobookmark:
            bookmarkctx = scmutil.revsingle(repo, ontobookmark)
            if not pushrebase and bookmarkctx.node() != targetnode:
                raise error.Abort(
                    _("destination parent does not match destination bookmark")
                )
        elif pushrebase:
            raise error.Abort(_("must specify destination bookmark for pushrebase"))

        if pushrebase:
            ontonode = bookmarkctx.node()
            cl = repo.changelog
            if cl.isancestor(originalparentnode, ontonode):
                targetctx = bookmarkctx
            elif cl.isancestor(ontonode, originalparentnode):
                targetctx = repo[originalparentnode]
            else:
                raise error.Abort(
                    _(
                        "destination bookmark is not ancestor or descendant of commit parent"
                    )
                )

        added, replacements = request.pushonto(targetctx)

        if len(added) > 1:
            # We always create a single commit.
            error.Abort(_("more than one commit was created"))

        if replacements:
            # We always create a new commit and therefore, cannot have any
            # replacements.
            error.Abort(_("new commit cannot replace any commit"))

        node = added[0]

        if ontobookmark:
            bookmarks.pushbookmark(repo, ontobookmark, bookmarkctx.hex(), node)

        return node
