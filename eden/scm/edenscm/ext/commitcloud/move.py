# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

from edenscm import error, node as nodemod, progress, templatefilters, util
from edenscm.i18n import _

from . import service, util as ccutil


srcdstworkspaceopts = [
    ("s", "source", "", _("short name for the source user workspace")),
    ("d", "destination", "", _("short name for the destination user workspace")),
    (
        "",
        "raw-source",
        "",
        _("raw source workspace name (e.g. 'user/<username>/<workspace>') (ADVANCED)"),
    ),
    (
        "",
        "raw-destination",
        "",
        _(
            "raw destination workspace name (e.g. 'user/<username>/<workspace>') (ADVANCED)"
        ),
    ),
]

moveopts = [
    ("r", "rev", [], _("revisions to hide (hash or prefix only)")),
    ("B", "bookmark", [], _("bookmarks to remove")),
    ("", "remotebookmark", [], _("remote bookmarks to remove")),
]


def moveorhide(
    repo,
    workspacename,
    revs,
    bookmarks,
    remotebookmarks,
    destination=None,
    dry_run=False,
    keep=False,
):
    reponame = ccutil.getreponame(repo)
    ui = repo.ui

    with progress.spinner(ui, _("fetching commit cloud workspace")):
        serv = service.get(ui)
        slinfo = serv.getsmartlog(reponame, workspacename, repo, 0)
        firstpublic, revdag = serv.makedagwalker(slinfo, repo)
        cloudrefs = serv.getreferences(reponame, workspacename, 0)

    nodeinfos = slinfo.nodeinfos
    dag = slinfo.dag
    drafts = set(slinfo.draft)
    hexdrafts = set(nodemod.hex(d) for d in slinfo.draft)

    removenodes = set()

    for rev in revs:
        if rev in hexdrafts:
            removenodes.add(nodemod.bin(rev))
        else:
            candidate = None
            for hexdraft in hexdrafts:
                if hexdraft.startswith(rev):
                    if candidate is None:
                        candidate = hexdraft
                    else:
                        raise error.Abort(_("ambiguous commit hash prefix: %s") % rev)
            if candidate is None:
                raise error.Abort(_("commit not in workspace: %s") % rev)
            removenodes.add(nodemod.bin(candidate))

    # Find the bookmarks we need to remove
    removebookmarks = set()
    for bookmark in bookmarks:
        kind, pattern, matcher = util.stringmatcher(bookmark)
        if kind == "literal":
            if pattern not in cloudrefs.bookmarks:
                raise error.Abort(_("bookmark not in workspace: %s") % pattern)
            removebookmarks.add(pattern)
        else:
            for bookmark in cloudrefs.bookmarks:
                if matcher(bookmark):
                    removebookmarks.add(bookmark)

    # Find the remote bookmarks we need to remove
    removeremotes = set()
    for remote in remotebookmarks:
        kind, pattern, matcher = util.stringmatcher(remote)
        if kind == "literal":
            if pattern not in cloudrefs.remotebookmarks:
                raise error.Abort(_("remote bookmark not in workspace: %s") % pattern)
            removeremotes.add(remote)
        else:
            for remote in cloudrefs.remotebookmarks:
                if matcher(remote):
                    removeremotes.add(remote)

    # Find the heads and bookmarks we need to remove
    allremovenodes = dag.descendants(removenodes)
    removeheads = set(allremovenodes & map(nodemod.bin, cloudrefs.heads))
    for node in allremovenodes:
        removebookmarks.update(nodeinfos[node].bookmarks)

    # Find the heads we need to remove because we are removing the last bookmark
    # to it.
    remainingheads = set(
        set(map(nodemod.bin, cloudrefs.heads)) & dag.all() - removeheads
    )
    for bookmark in removebookmarks:
        node = nodemod.bin(cloudrefs.bookmarks[bookmark])
        info = nodeinfos.get(node)
        if node in remainingheads and info:
            if removebookmarks.issuperset(set(info.bookmarks)):
                remainingheads.discard(node)
                removeheads.add(node)

    # Find the heads we need to add to keep other commits visible
    addheads = (
        dag.parents(removenodes) - allremovenodes - dag.ancestors(remainingheads)
    ) & drafts

    operation = (
        "copying" if keep and destination else ("moving" if destination else "removing")
    )

    if removeheads:
        ui.status(_("%s heads:\n") % operation)
        for head in sorted(removeheads):
            hexhead = nodemod.hex(head)
            ui.status(
                "    %s  %s\n"
                % (hexhead[:12], templatefilters.firstline(nodeinfos[head].message))
            )

    if removebookmarks:
        ui.status(_("%s bookmarks:\n") % operation)
        for bookmark in sorted(removebookmarks):
            ui.status("    %s: %s\n" % (bookmark, cloudrefs.bookmarks[bookmark][:12]))

    if removeremotes:
        ui.status(_("%s remote bookmarks:\n") % operation)
        for remote in sorted(removeremotes):
            ui.status("    %s: %s\n" % (remote, cloudrefs.remotebookmarks[remote][:12]))

    if addheads and not keep:
        ui.status(_("adding heads:\n"))
        for head in sorted(addheads):
            hexhead = nodemod.hex(head)
            ui.status(
                "    %s  %s\n"
                % (hexhead[:12], templatefilters.firstline(nodeinfos[head].message))
            )

    removeheadsancestors = (dag.ancestors(removeheads) - removeheads) & drafts

    # Hexify all the head, as cloudrefs works with hex strings.
    removeheads = list(map(nodemod.hex, removeheads))
    addheads = list(map(nodemod.hex, addheads))
    removeheadsancestors = set(map(nodemod.hex, removeheadsancestors))

    if removeheads or addheads or removebookmarks or removeremotes:
        if dry_run:
            ui.status(_("not updating cloud workspace: --dry-run specified\n"))
            return 0

        if destination:
            removebookmarksset = set(removebookmarks)
            destcloudrefs = serv.getreferences(reponame, destination, 0)
            destheadsset = set(destcloudrefs.heads)

            # HEADS changes
            newheads = [head for head in removeheads if head not in destheadsset]
            # some previous heads may not be actually heads anymore but rather part of a longer stack
            oldheads = [
                head for head in destcloudrefs.heads if head in removeheadsancestors
            ]

            # BOOKMARKS changes
            newbookmarks = {
                name: node
                for name, node in cloudrefs.bookmarks.items()
                if name in removebookmarksset
            }
            # new bookmarks may override values of these bookmarks, so they have to be removed
            oldbookmarks = [
                name
                for name, value in destcloudrefs.bookmarks.items()
                if name in removebookmarksset
            ]

            # REMOTE BOOKMARKS changes
            newremotebookmarks = {
                name: node
                for name, node in cloudrefs.remotebookmarks.items()
                if name in removeremotes
            }
            # new remote bookmarks may override values of these remote bookmarks, so they have to be removed
            oldremotebookmarks = [
                name
                for name, node in destcloudrefs.remotebookmarks.items()
                if name in removeremotes
            ]

            with progress.spinner(ui, _("updating destination workspace")):
                res, _refs = serv.updatereferences(
                    reponame,
                    destination,
                    destcloudrefs.version,
                    newheads=newheads,
                    oldheads=oldheads,
                    newbookmarks=newbookmarks,
                    oldbookmarks=oldbookmarks,
                    newremotebookmarks=newremotebookmarks,
                    oldremotebookmarks=oldremotebookmarks,
                )
                if not res:
                    raise error.Abort(
                        _(
                            "conflict: the workspace '%s' was modified at the same time by another operation, please retry"
                        )
                        % destination
                    )

        if not keep:
            with progress.spinner(ui, _("updating commit cloud workspace")):
                res, _refs = serv.updatereferences(
                    reponame,
                    workspacename,
                    cloudrefs.version,
                    oldheads=list(removeheads),
                    newheads=list(addheads),
                    oldbookmarks=list(removebookmarks),
                    oldremotebookmarks=list(removeremotes),
                )
                if not res:
                    raise error.Abort(
                        _(
                            "conflict: the workspace '%s' has been modified during this operation by another participant\n"
                            "please, retry!"
                        )
                        % destination
                    )
        return 1
    else:
        ui.status(_("nothing to change\n"))
        return 0
