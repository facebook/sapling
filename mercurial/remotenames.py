# remotenames.py
#
# Copyright 2017 Augie Fackler <raf@durin42.com>
# Copyright 2017 Sean Farley <sean@farley.io>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from .node import hex

def pullremotenames(localrepo, remoterepo):
    """
    pulls bookmarks and branches information of the remote repo during a
    pull or clone operation.
    localrepo is our local repository
    remoterepo is the peer instance
    """
    remotepath = remoterepo.url()
    bookmarks = remoterepo.listkeys('bookmarks')
    # on a push, we don't want to keep obsolete heads since
    # they won't show up as heads on the next pull, so we
    # remove them here otherwise we would require the user
    # to issue a pull to refresh the storage
    bmap = {}
    repo = localrepo.unfiltered()
    for branch, nodes in remoterepo.branchmap().iteritems():
        bmap[branch] = []
        for node in nodes:
            if node in repo and not repo[node].obsolete():
                bmap[branch].append(hex(node))

    # writing things to ui till the time we import the saving functionality
    ui = localrepo.ui
    ui.write("\nRemotenames info\npath: %s\n" % remotepath)
    ui.write("Bookmarks:\n")
    for bm, node in bookmarks.iteritems():
        ui.write("%s: %s\n" % (bm, node))
    ui.write("Branches:\n")
    for branch, node in bmap.iteritems():
        ui.write("%s: %s\n" % (branch, node))
    ui.write("\n")
