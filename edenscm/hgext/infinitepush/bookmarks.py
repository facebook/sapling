# Copyright 2016-2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import json
import struct

from edenscm.mercurial import error, extensions, node as nodemod
from edenscm.mercurial.i18n import _


def remotebookmarksenabled(ui):
    return "remotenames" in extensions._extensions and ui.configbool(
        "remotenames", "bookmarks"
    )


def readremotebookmarks(ui, repo, other):
    if remotebookmarksenabled(ui):
        remotenamesext = extensions.find("remotenames")
        remotepath = remotenamesext.activepath(repo.ui, other)
        result = {}
        # Let's refresh remotenames to make sure we have it up to date
        # Seems that `repo.names['remotebookmarks']` may return stale bookmarks
        # and it results in deleting scratch bookmarks. Our best guess how to
        # fix it is to use `clearnames()`
        repo._remotenames.clearnames()
        for remotebookmark in repo.names["remotebookmarks"].listnames(repo):
            path, bookname = remotenamesext.splitremotename(remotebookmark)
            if path == remotepath and repo._scratchbranchmatcher.match(bookname):
                nodes = repo.names["remotebookmarks"].nodes(repo, remotebookmark)
                if nodes:
                    result[bookname] = nodemod.hex(nodes[0])
        return result
    else:
        return {}


def saveremotebookmarks(repo, newbookmarks, remote):
    remotenamesext = extensions.find("remotenames")
    remotepath = remotenamesext.activepath(repo.ui, remote)
    bookmarks = {}
    remotenames = remotenamesext.readremotenames(repo)
    for hexnode, nametype, remote, rname in remotenames:
        if remote != remotepath:
            continue
        if nametype == "bookmarks":
            if rname in newbookmarks:
                # It's possible if we have a normal bookmark that matches
                # scratch branch pattern. In this case just use the current
                # bookmark node
                del newbookmarks[rname]
            bookmarks[rname] = hexnode

    for bookmark, hexnode in newbookmarks.iteritems():
        bookmarks[bookmark] = hexnode
    remotenamesext.saveremotenames(repo, {remotepath: bookmarks})


def savelocalbookmarks(repo, bookmarks):
    if not bookmarks:
        return
    with repo.wlock(), repo.lock(), repo.transaction("bookmark") as tr:
        changes = []
        for scratchbook, node in bookmarks.iteritems():
            changectx = repo[node]
            changes.append((scratchbook, changectx.node()))
        repo._bookmarks.applychanges(repo, tr, changes)


def deleteremotebookmarks(ui, repo, path, names):
    """Prune remote names by removing the bookmarks we don't want anymore,
    then writing the result back to disk
    """
    remotenamesext = extensions.find("remotenames")

    # remotename format is:
    # (node, nametype ("bookmarks"), remote, name)
    nametype_idx = 1
    remote_idx = 2
    name_idx = 3
    remotenames = [
        remotename
        for remotename in remotenamesext.readremotenames(repo)
        if remotename[remote_idx] == path
    ]
    remote_bm_names = [
        remotename[name_idx]
        for remotename in remotenames
        if remotename[nametype_idx] == "bookmarks"
    ]

    for name in names:
        if name not in remote_bm_names:
            raise error.Abort(
                _("infinitepush bookmark '{}' does not exist " "in path '{}'").format(
                    name, path
                )
            )

    bookmarks = {}
    for node, nametype, remote, name in remotenames:
        if nametype == "bookmarks" and name not in names:
            bookmarks[name] = node

    remotenamesext.saveremotenames(repo, {path: bookmarks})


def encodebookmarks(bookmarks):
    encoded = {}
    for bookmark, node in bookmarks.iteritems():
        encoded[bookmark] = node
    dumped = json.dumps(encoded)
    result = struct.pack(">i", len(dumped)) + dumped
    return result


def decodebookmarks(stream):
    sizeofjsonsize = struct.calcsize(">i")
    size = struct.unpack(">i", stream.read(sizeofjsonsize))[0]
    unicodedict = json.loads(stream.read(size))
    # python json module always returns unicode strings. We need to convert
    # it back to bytes string
    result = {}
    for bookmark, node in unicodedict.iteritems():
        bookmark = bookmark.encode("ascii")
        node = node.encode("ascii")
        result[bookmark] = node
    return result
