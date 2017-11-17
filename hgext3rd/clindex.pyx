# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""alternative changelog index

This extension replaces certain parts of changelog index algorithms to make it
more efficient when changelog is large.
"""

from __future__ import absolute_import

from mercurial import (
    changelog,
    error,
    extensions,
    policy,
    revlog,
)

origindex = policy.importmod('parsers').index

# cdef is important for performance because it avoids dict lookups:
# - `self._origindex` becomes `some_c_struct_pointer->_origindex`
# - `__getitem__`, `__len__` will be using `PyMappingMethods` APIs

cdef class clindex(object):
    cdef _origindex

    def __init__(self, data, inlined):
        assert not inlined
        self._origindex = origindex(data, inlined)

    def ancestors(self, *revs):
        return self._origindex.ancestors(*revs)

    def commonancestorsheads(self, *revs):
        return self._origindex.commonancestorsheads(*revs)

    def __getitem__(self, int rev):
        return self._origindex[rev]

    def computephasesmapsets(self, roots):
        return self._origindex.computephasesmapsets(roots)

    def reachableroots2(self, int minroot, heads, roots, includepath):
        return self._origindex.reachableroots2(minroot, heads, roots,
                                               includepath)

    def headrevs(self):
        return self._origindex.headrevs()

    def headrevsfiltered(self, filtered):
        return self._origindex.headrevsfiltered(filtered)

    def deltachain(self, rev, stoprev, generaldelta):
        return self._origindex.deltachain(rev, stoprev, generaldelta)

    def insert(self, int rev, entry):
        return self._origindex.insert(rev, entry)

    def partialmatch(self, hexnode):
        return self._origindex.partialmatch(hexnode)

    def __len__(self):
        return len(self._origindex)

    @property
    def nodemap(self):
        return nodemap(self._origindex)

cdef class nodemap(object):
    cdef _origindex

    def __init__(self, origindex):
        self._origindex = origindex

    def __getitem__(self, node):
        return self._origindex[node]

    def __setitem__(self, node, rev):
        self._origindex[node] = rev

    def __contains__(self, node):
        return node in self._origindex

    def get(self, node, default=None):
        return self._origindex.get(node, default)

def _parseindex(orig, self, data, inline):
    if inline:
        # clindex does not support inline
        return orig(self, data, inline)
    index = clindex(data, inline)
    return index, index.nodemap, None

def _changeloginit(orig, self, *args, **kwargs):
    with extensions.wrappedfunction(revlog.revlogio, 'parseindex', _parseindex):
        orig(self, *args, **kwargs)

def uisetup(ui):
    extensions.wrapfunction(changelog.changelog, '__init__', _changeloginit)
