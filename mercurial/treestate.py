# Copyright Facebook, Inc. 2018
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""tree-based dirstate combined with other (ex. fsmonitor) states"""

from __future__ import absolute_import


class treestatemap(object):
    """a drop-in replacement for dirstate._map, with more abilities like also
    track fsmonitor state.
    """

    def __init__(self, ui, opener, root):
        raise NotImplementedError

    @property
    def copymap(self):
        raise NotImplementedError

    def clear(self):
        raise NotImplementedError

    def iteritems(self):
        raise NotImplementedError

    def __len__(self):
        raise NotImplementedError

    def get(self, key, default=None):
        raise NotImplementedError

    def __contains__(self, key):
        raise NotImplementedError

    def __getitem__(self, key):
        result = self.get(key)
        if result is None:
            raise KeyError(key)
        return result

    def keys(self):
        raise NotImplementedError

    def preload(self):
        raise NotImplementedError

    def addfile(self, f, oldstate, state, mode, size, mtime):
        raise NotImplementedError

    def removefile(self, f, oldstate, size):
        raise NotImplementedError

    def dropfile(self, f, oldstate):
        raise NotImplementedError

    def clearambiguoustimes(self, files, now):
        raise NotImplementedError

    def nonnormalentries(self):
        raise NotImplementedError

    @property
    def filefoldmap(self):
        raise NotImplementedError

    def hastrackeddir(self, d):
        raise NotImplementedError

    def hasdir(self, d):
        raise NotImplementedError

    def parents(self):
        raise NotImplementedError

    def setparents(self, p1, p2):
        raise NotImplementedError

    def read(self):
        raise NotImplementedError

    def write(self, st, now):
        raise NotImplementedError

    @property
    def nonnormalset(self):
        raise NotImplementedError

    @property
    def otherparentset(self):
        raise NotImplementedError

    @property
    def identity(self):
        raise NotImplementedError

    @property
    def dirfoldmap(self):
        raise NotImplementedError
