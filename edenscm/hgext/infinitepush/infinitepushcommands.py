# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""
config::

    [infinitepush]
    # limit number of files in the node metadata. This is to make sure we don't
    # waste too much space on huge codemod commits.
    metadatafilelimit = 100
"""

from __future__ import absolute_import

import json

from edenscm.mercurial import (
    copies as copiesmod,
    encoding,
    error,
    hg,
    patch,
    registrar,
    scmutil,
    util,
)
from edenscm.mercurial.i18n import _

# Mercurial
from edenscm.mercurial.node import bin

from . import common, server


cmdtable = {}
command = registrar.command(cmdtable)


@command(
    "debugfillinfinitepushmetadata", [("", "node", [], "node to fill metadata for")]
)
def debugfillinfinitepushmetadata(ui, repo, **opts):
    """Special command that fills infinitepush metadata for a node
    """

    nodes = opts["node"]
    if not nodes:
        raise error.Abort(_("nodes are not specified"))

    filelimit = ui.configint("infinitepush", "metadatafilelimit", 100)
    nodesmetadata = {}
    for node in nodes:
        index = repo.bundlestore.index
        if not bool(index.getbundle(node)):
            raise error.Abort(_("node %s is not found") % node)

        if node not in repo:
            newbundlefile = server.downloadbundle(repo, bin(node))
            bundlepath = "bundle:%s+%s" % (repo.root, newbundlefile)
            bundlerepo = hg.repository(ui, bundlepath)
            repo = bundlerepo

        p1 = repo[node].p1().node()
        diffopts = patch.diffallopts(ui, {})
        match = scmutil.matchall(repo)
        chunks = patch.diff(repo, p1, node, match, None, diffopts, relroot="")
        difflines = util.iterlines(chunks)

        states = "modified added removed deleted unknown ignored clean".split()
        status = repo.status(p1, node)
        status = zip(states, status)

        filestatus = {}
        for state, files in status:
            for f in files:
                filestatus[f] = state

        diffstat = patch.diffstatdata(difflines)
        changed_files = {}
        copies = copiesmod.pathcopies(repo[p1], repo[node])
        for filename, adds, removes, isbinary in diffstat[:filelimit]:
            # use special encoding that allows non-utf8 filenames
            filename = encoding.jsonescape(filename, paranoid=True)
            changed_files[filename] = {
                "adds": adds,
                "removes": removes,
                "isbinary": isbinary,
                "status": filestatus.get(filename, "unknown"),
            }
            if filename in copies:
                changed_files[filename]["copies"] = copies[filename]

        output = {}
        output["changed_files"] = changed_files
        if len(diffstat) > filelimit:
            output["changed_files_truncated"] = True
        nodesmetadata[node] = output

    with index:
        for node, metadata in nodesmetadata.iteritems():
            dumped = json.dumps(metadata, sort_keys=True)
            index.saveoptionaljsonmetadata(node, dumped)


def _resolvescratchbookmark(ui, scratchbookmarkname):
    if not scratchbookmarkname:
        raise error.Abort(_("scratch bookmark name is required"))

    if not common.scratchbranchmatcher(ui).match(scratchbookmarkname):
        raise error.Abort(_("invalid scratch bookmark name"))

    return scratchbookmarkname


def _resolvetargetnode(repo, rev):
    index = repo.bundlestore.index
    targetnode = index.getnodebyprefix(rev)
    if not targetnode:
        revs = scmutil.revrange(repo, [rev])
        if len(revs) != 1:
            raise error.Abort(
                _("must specify exactly one target commit for scratch bookmark")
            )

        targetnode = repo[revs.last()].hex()

    return targetnode


@command(
    "debugcreatescratchbookmark",
    [
        ("r", "rev", "", _("target commit for scratch bookmark"), _("REV")),
        ("B", "bookmark", "", _("scratch bookmark name"), _("BOOKMARK")),
    ],
    _("-r REV -B BOOKMARK"),
)
def debugcreatescratchbookmark(ui, repo, *args, **opts):
    """create scratch bookmark on the specified revision
    """
    if not common.isserver(ui):
        raise error.Abort(
            _("scratch bookmarks can only be created on an infinitepush server")
        )

    scratchbookmarkname = _resolvescratchbookmark(ui, opts.get("bookmark"))
    index = repo.bundlestore.index
    with index:
        if index.getnode(scratchbookmarkname):
            raise error.Abort(
                _("scratch bookmark '%s' already exists") % scratchbookmarkname
            )

        targetnode = _resolvetargetnode(repo, opts.get("rev"))
        index.addbookmark(scratchbookmarkname, targetnode, False)


@command(
    "debugmovescratchbookmark",
    [
        ("r", "rev", "", _("target commit for scratch bookmark"), _("REV")),
        ("B", "bookmark", "", _("scratch bookmark name"), _("BOOKMARK")),
    ],
    _("-r REV -B BOOKMARK"),
)
def debugmovescratchbookmark(ui, repo, *args, **opts):
    """move existing scratch bookmark to the specified revision
    """
    if not common.isserver(ui):
        raise error.Abort(
            _("scratch bookmarks can only be moved on an infinitepush server")
        )

    scratchbookmarkname = _resolvescratchbookmark(ui, opts.get("bookmark"))
    index = repo.bundlestore.index
    with index:
        currentnode = index.getnode(scratchbookmarkname)
        if not currentnode:
            raise error.Abort(
                _("scratch bookmark '%s' does not exist") % scratchbookmarkname
            )

        targetnode = _resolvetargetnode(repo, opts.get("rev"))
        index.deletebookmarks([scratchbookmarkname])
        index.addbookmark(scratchbookmarkname, targetnode, False)
