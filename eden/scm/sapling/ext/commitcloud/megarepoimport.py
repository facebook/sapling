# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

from sapling import error
from sapling.ext import megarepo
from sapling.i18n import _

from . import service, sync, syncstate, util as ccutil, workspace


def validateimportparams(ui, repo, opts):
    destinationworkspace = opts.get("destination")
    rawdestinationworkspace = opts.get("raw_destination")
    destinationrepo = opts.get("destination_repo")

    if rawdestinationworkspace and destinationworkspace:
        raise error.Abort(
            "conflicting 'destination' and 'raw-destination' options provided"
        )
    elif rawdestinationworkspace:
        destinationworkspace = rawdestinationworkspace
    elif not destinationworkspace or destinationworkspace == ".":
        destinationworkspace = workspace.currentworkspace(repo)
    else:
        destinationworkspace = workspace.userworkspaceprefix(ui) + destinationworkspace

    if not destinationrepo or destinationrepo == ".":
        destinationrepo = ccutil.getreponame(repo)

    sourceworkspace = opts.get("source")
    rawsourceworkspace = opts.get("raw_source")
    sourcerepo = opts.get("source_repo")
    if rawsourceworkspace and sourceworkspace:
        raise error.Abort("conflicting 'source' and 'raw-source' options provided")
    elif rawsourceworkspace:
        sourceworkspace = rawsourceworkspace
    elif not sourceworkspace or sourceworkspace == ".":
        sourceworkspace = workspace.currentworkspace(repo)
    else:
        sourceworkspace = workspace.userworkspaceprefix(ui) + sourceworkspace

    if not sourcerepo or sourcerepo == ".":
        sourcerepo = ccutil.getreponame(repo)
    if sourceworkspace == destinationworkspace and sourcerepo == destinationrepo:
        raise error.Abort(
            _(
                "the source workspace '%s' and the destination workspace '%s' are the same"
            )
            % (sourceworkspace, destinationworkspace)
        )

    return sourceworkspace, destinationworkspace, sourcerepo, destinationrepo


def fetchworkspaces(
    ui, repo, sourceworkspace, destinationworkspace, sourcerepo, destinationrepo, serv
):

    # Validate source workspace
    srcinfo = serv.getworkspace(sourcerepo, sourceworkspace)
    if not srcinfo:
        raise error.Abort(_("source workspace '%s' does not exist") % sourceworkspace)

    # Validate destination workspace
    dstinfo = serv.getworkspace(destinationrepo, destinationworkspace)
    if not dstinfo:
        raise error.Abort(
            _("destination workspace '%s' does not exist") % destinationworkspace
        )


def translateandpull(
    ui, repo, sourceworkspace, destinationworkspace, sourcerepo, destinationrepo, serv
):

    cloudrefs = serv.getreferences(
        sourcerepo,
        sourceworkspace,
        0,
        clientinfo=service.makeclientinfo(
            repo, syncstate.SyncState(repo, sourceworkspace)
        ),
    )

    # Get the list of commits to pull
    headsdates = cloudrefs.headdates
    translatequeue = {}
    for head in headsdates:
        if megarepo.may_need_xrepotranslate(repo, head):
            translatequeue[head] = headsdates[head]

    # Translate heads
    newheads = {}
    for head in translatequeue:
        newhead = megarepo.xrepotranslate(repo, head).hex()
        newheads[newhead] = translatequeue[head]

    remotepath = ccutil.getremotepath(ui)

    headgroups = sync.partitionheads(ui, newheads)
    sync.pullheadgroups(repo, remotepath, headgroups)

    # Translate bookmarks
    bookmarks = cloudrefs.bookmarks
    bookmarkstotranslate = {}
    for bookmark in bookmarks:
        if megarepo.may_need_xrepotranslate(repo, bookmarks[bookmark]):
            bookmarkstotranslate[bookmark] = bookmarks[bookmark]

    newbookmarks = []
    for bookmark in bookmarkstotranslate:
        newbookmarknode = megarepo.xrepotranslate(repo, bookmarkstotranslate[bookmark])
        newbookmarks.append((bookmark, newbookmarknode))

    with repo.wlock(), repo.lock(), repo.transaction("cloudsync") as tr:
        repo._bookmarks.applychanges(repo, tr, newbookmarks, warnoverwrite=True)
