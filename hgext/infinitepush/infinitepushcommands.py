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

from mercurial import (
    copies as copiesmod,
    encoding,
    error,
    hg,
    patch,
    registrar,
    scmutil,
    util,
)
from mercurial.i18n import _

# Mercurial
from mercurial.node import bin

from . import backupcommands, common


cmdtable = backupcommands.cmdtable
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
            newbundlefile = common.downloadbundle(repo, bin(node))
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
