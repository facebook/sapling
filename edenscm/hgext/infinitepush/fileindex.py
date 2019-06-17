# Infinite push
#
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""
    [infinitepush]
    # Server-side option. Used only if indextype=disk.
    # Filesystem path to the index store
    indexpath = PATH
"""

import os
import posixpath

from edenscm.mercurial import error, pycompat, util
from edenscm.mercurial.i18n import _


if pycompat.iswindows:

    def _normalizepath(path):
        # Remove known characters that is disallowed by Windows.
        # ":" can appear in some tests where the path is joined like:
        # "C:\\repo1\\.hg\\scratchbranches\\index\\bookmarkmap\\infinitepush/backups/test/HOSTNAME/C:\\repo2/heads"
        return path.replace(":", "")


else:

    def _normalizepath(path):
        return path


class fileindex(object):
    """File-based backend for infinitepush index.

    This is a context manager.  All write operations should use:

        with index:
            index.addbookmark(...)
            ...
    """

    def __init__(self, repo):
        self._repo = repo
        root = repo.ui.config("infinitepush", "indexpath")
        if not root:
            root = os.path.join("scratchbranches", "index")

        self._nodemap = os.path.join(root, "nodemap")
        self._bookmarkmap = os.path.join(root, "bookmarkmap")
        self._metadatamap = os.path.join(root, "nodemetadatamap")
        self._lock = None

    def __enter__(self):
        self._lock = self._repo.wlock()
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        if self._lock:
            self._lock.__exit__(exc_type, exc_val, exc_tb)

    def addbundle(self, bundleid, nodesctx):
        """Record a bundleid containing the given nodes."""

        for node in nodesctx:
            nodepath = os.path.join(self._nodemap, node.hex())
            self._write(nodepath, bundleid)

    def addbookmark(self, bookmark, node, _isbackup):
        """Record a bookmark pointing to a particular node."""
        bookmarkpath = os.path.join(self._bookmarkmap, bookmark)
        self._write(bookmarkpath, node)

    def addmanybookmarks(self, bookmarks, isbackup):
        """Record the contents of the ``bookmarks`` dict as bookmarks."""
        for bookmark, node in bookmarks.items():
            self.addbookmark(bookmark, node, isbackup)

    def deletebookmarks(self, patterns):
        """Delete all bookmarks that match any of the patterns in ``patterns``."""
        for pattern in patterns:
            for bookmark, _node in self._listbookmarks(pattern):
                bookmarkpath = os.path.join(self._bookmarkmap, bookmark)
                self._delete(bookmarkpath)

    def getbundle(self, node):
        """Get the bundleid for a bundle that contains the given node."""
        nodepath = os.path.join(self._nodemap, node)
        return self._read(nodepath)

    def getnodebyprefix(self, hashprefix):
        """Get the node that matches the given hash prefix.

        If there is no match, returns None.

        If there are multiple matches, raises an exception."""
        vfs = self._repo.localvfs
        if not vfs.exists(self._nodemap):
            return None

        files = vfs.listdir(self._nodemap)
        nodefiles = filter(lambda n: n.startswith(hashprefix), files)

        if not nodefiles:
            return None

        if len(nodefiles) > 1:
            raise error.Abort(
                _(
                    "ambiguous identifier '%s'\n"
                    "suggestion: provide longer commithash prefix"
                )
                % hashprefix
            )

        return nodefiles[0]

    def getnode(self, bookmark):
        """Get the node for the given bookmark."""
        bookmarkpath = os.path.join(self._bookmarkmap, bookmark)
        return self._read(bookmarkpath)

    def getbookmarks(self, pattern):
        """Get all bookmarks that match the pattern."""
        return sorted(self._listbookmarks(pattern))

    def saveoptionaljsonmetadata(self, node, jsonmetadata):
        """Save optional metadata for the given node."""
        vfs = self._repo.localvfs
        vfs.write(os.path.join(self._metadatamap, node), jsonmetadata)

    def _listbookmarks(self, pattern):
        if pattern.endswith("*"):
            pattern = "re:^" + pattern[:-1] + ".*"
        kind, pat, matcher = util.stringmatcher(pattern)
        prefixlen = len(self._bookmarkmap) + 1
        for dirpath, _dirs, books in self._repo.localvfs.walk(self._bookmarkmap):
            for book in books:
                bookmark = posixpath.join(dirpath, book)[prefixlen:]
                if not matcher(bookmark):
                    continue
                yield bookmark, self._read(os.path.join(dirpath, book))

    def _write(self, path, value):
        vfs = self._repo.localvfs
        path = _normalizepath(path)
        dirname = vfs.dirname(path)
        if not vfs.exists(dirname):
            vfs.makedirs(dirname)

        vfs.write(path, value)

    def _read(self, path):
        vfs = self._repo.localvfs
        path = _normalizepath(path)
        if not vfs.exists(path):
            return None
        return vfs.read(path)

    def _delete(self, path):
        vfs = self._repo.localvfs
        path = _normalizepath(path)
        if not vfs.exists(path):
            return
        return vfs.unlink(path)
