# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Copyright 2013 Mercurial Contributors
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from .. import (
    bookmarks as bookmarksmod,
    cmdutil,
    error,
    hg,
    lock as lockmod,
    merge,
    node as nodemod,
    repair,
    scmutil,
)
from ..i18n import _
from .cmdtable import command


nullid = nodemod.nullid
release = lockmod.release


def checklocalchanges(repo, force=False, excsuffix=""):
    cmdutil.checkunfinished(repo)
    s = repo.status()
    if not force:
        if s.modified or s.added or s.removed or s.deleted:
            _("local changes found")  # i18n tool detection
            raise error.Abort(_("local changes found" + excsuffix))
    return s


def _findupdatetarget(repo, nodes):
    unode, p2 = repo.changelog.parents(nodes[0])
    currentbranch = repo[None].branch()

    if currentbranch != repo[unode].branch():
        pwdir = "parents(wdir())"
        revset = "max(((parents(%ln::%r) + %r) - %ln::%r) and branch(%s))"
        branchtarget = list(
            repo.nodes(revset, nodes, pwdir, pwdir, nodes, pwdir, currentbranch)
        )
        if branchtarget:
            unode = branchtarget[0]

    return unode


def strip(ui, repo, revs, update=True, backup=True, force=None, bookmarks=None):
    with repo.wlock(), repo.lock():

        if update:
            checklocalchanges(repo, force=force)
            urev = _findupdatetarget(repo, revs)
            hg.clean(repo, urev)
            repo.dirstate.write(repo.currenttransaction())

        repair.strip(ui, repo, revs, backup)

        if bookmarks:
            with repo.transaction("strip-bookmarks") as tr:
                bookmarksmod.delete(repo, tr, bookmarks)
            for bookmark in sorted(bookmarks):
                ui.write(_("bookmark '%s' deleted\n") % bookmark)


@command(
    "debugstrip",
    [
        ("r", "rev", [], _("revision to strip"), _("REV")),
        (
            "f",
            "force",
            None,
            _("force removal, discarding uncommitted changes without backup"),
        ),
        ("", "no-backup", None, _("do not keep a backup of the removed commits")),
        ("k", "keep", None, _("do not modify working directory during strip")),
        ("B", "bookmark", [], _("remove revs only reachable from given bookmark")),
    ],
    _("hg strip [-k] [-f] [-B bookmark] [-r] REV..."),
)
def stripcmd(ui, repo, *revs, **opts):
    """strip commits and all their descendants from the repository

    The debugstrip command removes the specified commits and all their
    descendants. If the working directory has uncommitted changes, the
    operation is aborted unless the --force flag is supplied, in which
    case changes will be discarded.

    If a parent of the working directory is stripped, then the working
    directory will automatically be updated to the most recent
    available ancestor of the stripped parent after the operation
    completes.

    Any stripped commits are stored in ``.hg/strip-backup`` as a
    bundle (see :hg:`help bundle` and :hg:`help unbundle`). They can
    be restored by running :hg:`unbundle .hg/strip-backup/BUNDLE`,
    where BUNDLE is the bundle file created by the strip. Note that
    the local revision numbers will in general be different after the
    restore.

    Use the --no-backup option to discard the backup bundle once the
    operation completes.

    Strip is not a history-rewriting operation and can be used on
    commits in the public phase. But if the stripped commits have
    been pushed to a remote repository you will likely pull them again.

    Return 0 on success.
    """
    backup = True
    if opts.get("no_backup"):
        backup = False

    cl = repo.changelog
    revs = list(revs) + opts.get("rev")
    revs = set(scmutil.revrange(repo, revs))
    nodes = set(cl.tonodes(revs))

    with repo.wlock():
        bookmarks = set(opts.get("bookmark"))
        if bookmarks:
            nodes.update(cl.tonodes(bookmarksmod.reachablerevs(repo, bookmarks)))
            if not nodes:
                # No revs are reachable exclusively from these bookmarks, just
                # delete the bookmarks.
                with repo.lock(), repo.transaction("strip-bookmarks") as tr:
                    bookmarksmod.delete(repo, tr, bookmarks)
                for bookmark in sorted(bookmarks):
                    ui.write(_("bookmark '%s' deleted\n") % bookmark)

        if not nodes:
            raise error.Abort(_("empty revision set"))

        strippednodes = cl.dag.descendants(nodes)
        rootnodes = set(cl.dag.roots(strippednodes))

        update = False
        # if one of the wdir parent is stripped we'll need
        # to update away to an earlier revision
        for p in repo.dirstate.parents():
            if p != nullid and p in strippednodes:
                update = True
                break

        revs = sorted(rootnodes)
        if update and opts.get("keep"):
            urev = _findupdatetarget(repo, revs)
            uctx = repo[urev]

            # only reset the dirstate for files that would actually change
            # between the working context and uctx
            descendantnodes = repo.nodes("%s::." % uctx.rev())
            changedfiles = []
            for n in descendantnodes:
                # blindly reset the files, regardless of what actually changed
                changedfiles.extend(repo[n].files())

            # reset files that only changed in the dirstate too
            dirstate = repo.dirstate
            dirchanges = [f for f in dirstate if dirstate[f] != "n"]
            changedfiles.extend(dirchanges)

            repo.dirstate.rebuild(urev, uctx.manifest(), changedfiles)
            repo.dirstate.write(repo.currenttransaction())

            # clear resolve state
            merge.mergestate.clean(repo, repo["."].node())

            update = False

        strip(
            ui,
            repo,
            revs,
            backup=backup,
            update=update,
            force=opts.get("force"),
            bookmarks=bookmarks,
        )

    return 0
