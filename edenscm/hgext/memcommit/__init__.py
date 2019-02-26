# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""make commits without a working copy

With this extension enabled, Mercurial provides a command i.e. `memcommit` to
make commits to a repository without requiring a working copy.

::
    [memcommit]
    # allow creating commits with no parents.
    allowunrelatedroots = False
"""

from __future__ import absolute_import

from edenscm.mercurial import bookmarks, error, registrar, scmutil
from edenscm.mercurial.i18n import _

from . import commitdata
from ..pushrebase.stackpush import pushrequest


configtable = {}
configitem = registrar.configitem(configtable)
configitem("memcommit", "allowunrelatedroots", default=False)

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


@command("^memcommit", [], _("hg memcommit"))
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
    """

    params = commitdata.params.deserialize(ui.fin)
    _memcommit(repo, params)


def _memcommit(repo, params):
    """create a new commit in the repo based on the params

    Isolating this method allows easy wrapping by other extensions like hgsql.
    """

    with repo.wlock(), repo.lock(), repo.transaction("memcommit"):
        request = pushrequest.frommemcommit(repo, params)
        p1node = request.stackparentnode

        destination = params.destination
        pushrebase = destination.pushrebase
        ontobookmark = destination.bookmark
        if ontobookmark:
            bookmarkctx = scmutil.revsingle(repo, ontobookmark)
            if not pushrebase and bookmarkctx.node() != p1node:
                raise error.Abort(
                    _("destination parent does not match destination bookmark")
                )
        elif pushrebase:
            raise error.Abort(_("must specify destination bookmark for pushrebase"))

        ontoctx = repo[p1node]
        if pushrebase:
            ontonode = bookmarkctx.node()
            cl = repo.changelog
            if cl.isancestor(p1node, ontonode):
                ontoctx = bookmarkctx
            elif not cl.isancestor(ontonode, p1node):
                raise error.Abort(
                    _(
                        "destination bookmark is not ancestor or descendant of commit parent"
                    )
                )

        added, replacements = request.pushonto(ontoctx)

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
