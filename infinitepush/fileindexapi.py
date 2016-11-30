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

from indexapi import (
    indexapi,
)

from mercurial import util

class fileindexapi(indexapi):
    def __init__(self, repo):
        super(fileindexapi, self).__init__()
        self._repo = repo
        root = repo.ui.config('infinitepush', 'indexpath')
        if not root:
            root = os.path.join('scratchbranches', 'index')

        self._nodemap = os.path.join(root, 'nodemap')
        self._bookmarkmap = os.path.join(root, 'bookmarkmap')

    def addbundle(self, bundleid, nodes):
        for node in nodes:
            nodepath = os.path.join(self._nodemap, node)
            self._write(nodepath, bundleid)

    def addbookmark(self, bookmark, node):
        bookmarkpath = os.path.join(self._bookmarkmap, bookmark)
        self._write(bookmarkpath, node)

    def addbookmarkandbundle(self, bundleid, nodes, bookmark, bookmarknode):
        self.addbookmark(bookmark, bookmarknode)
        self.addbundle(bundleid, nodes)

    def deletebookmarks(self, patterns, commit):
        """`commit` is ignored
        """
        for pattern in patterns:
            for bookmark, _ in self._listbookmarks(pattern):
                bookmarkpath = os.path.join(self._bookmarkmap, bookmark)
                self._delete(bookmarkpath)

    def getbundle(self, node):
        nodepath = os.path.join(self._nodemap, node)
        return self._read(nodepath)

    def getnode(self, bookmark):
        bookmarkpath = os.path.join(self._bookmarkmap, bookmark)
        return self._read(bookmarkpath)

    def getbookmarks(self, query):
        return dict(self._listbookmarks(query))

    def _listbookmarks(self, pattern):
        if pattern.endswith('*'):
            pattern = 're:^' + pattern[:-1] + '.*'
        kind, pat, matcher = util.stringmatcher(pattern)
        prefixlen = len(self._bookmarkmap) + 1
        for dirpath, _, books in self._repo.vfs.walk(self._bookmarkmap):
            for book in books:
                bookmark = os.path.join(dirpath, book)[prefixlen:]
                if not matcher(bookmark):
                    continue
                yield bookmark, self._read(os.path.join(dirpath, book))

    def _write(self, path, value):
        vfs = self._repo.vfs
        dirname = vfs.dirname(path)
        if not vfs.exists(dirname):
            vfs.makedirs(dirname)

        vfs.write(path, value)

    def _read(self, path):
        vfs = self._repo.vfs
        if not vfs.exists(path):
            return None
        return vfs.read(path)

    def _delete(self, path):
        vfs = self._repo.vfs
        if not vfs.exists(path):
            return
        return vfs.unlink(path)
