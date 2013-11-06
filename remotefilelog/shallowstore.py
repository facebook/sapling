# shallowstore.py - shallow store for interacting with shallow repos
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial import util
from mercurial import store as storemod
import stat

def wrapstore(store):
    class shallowstore(store.__class__):
        def _walk(self, relpath, recurse, allfiles=False):
            """modifies walk to return non .i/.d files so streaming clones
            can send remotefilelog store/data files
            """
            path = self.path
            if relpath:
                path += '/' + relpath
            striplen = len(self.path) + 1
            l = []
            if self.rawvfs.isdir(path):
                visit = [path]
                readdir = self.rawvfs.readdir
                while visit:
                    p = visit.pop()
                    for f, kind, st in readdir(p, stat=True):
                        fp = p + '/' + f
                        if (kind == stat.S_IFREG and
                           (allfiles or f[-2:] in ('.d', '.i'))):
                            n = util.pconvert(fp[striplen:])
                            l.append((storemod.decodedir(n), n, st.st_size))
                        elif kind == stat.S_IFDIR and recurse:
                            visit.append(fp)
            l.sort()
            return l

        def datafiles(self):
            for a, b, size in self._walk('data', True, True):
                yield a, b, size

        def __contains__(self, path):
            # Assume it exists
            return True

    store.__class__ = shallowstore

    return store
