# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import hashlib
import struct

from bindings import revisionstore
from edenscm.mercurial.node import hex, nullid

from . import basepack, constants, shallowutil


# (filename hash, offset, size)
INDEXFORMAT0 = "!20sQQ"
INDEXENTRYLENGTH0 = struct.calcsize(INDEXFORMAT0)
INDEXFORMAT1 = "!20sQQII"
INDEXENTRYLENGTH1 = struct.calcsize(INDEXFORMAT1)
NODELENGTH = 20

NODEINDEXFORMAT = "!20sQ"
NODEINDEXENTRYLENGTH = struct.calcsize(NODEINDEXFORMAT)

# (node, p1, p2, linknode)
PACKFORMAT = "!20s20s20s20sH"
PACKENTRYLENGTH = 82

ENTRYCOUNTSIZE = 4

INDEXSUFFIX = ".histidx"
PACKSUFFIX = ".histpack"

ANC_NODE = 0
ANC_P1NODE = 1
ANC_P2NODE = 2
ANC_LINKNODE = 3
ANC_COPYFROM = 4


class historypackstore(basepack.basepackstore):
    INDEXSUFFIX = INDEXSUFFIX
    PACKSUFFIX = PACKSUFFIX

    def __init__(self, ui, path, deletecorruptpacks=False):
        super(historypackstore, self).__init__(
            ui, path, deletecorruptpacks=deletecorruptpacks
        )

    def getpack(self, path):
        return revisionstore.historypack(path)

    def getancestors(self, name, node, known=None):
        def func(pack):
            return pack.getancestors(name, node, known=known)

        for ancestors in self.runonpacks(func):
            return ancestors

        raise KeyError((name, hex(node)))

    def getnodeinfo(self, name, node):
        def func(pack):
            return pack.getnodeinfo(name, node)

        for nodeinfo in self.runonpacks(func):
            return nodeinfo

        raise KeyError((name, hex(node)))

    def add(self, filename, node, p1, p2, linknode, copyfrom):
        raise RuntimeError(
            "cannot add to historypackstore (%s:%s)" % (filename, hex(node))
        )

    def repackstore(self):
        revisionstore.repackincrementalhistpacks(self.path, self.path)


def makehistorypackstore(ui, path, deletecorruptpacks=False):
    if ui.configbool("remotefilelog", "userustpackstore", False):
        return revisionstore.historypackstore(path, deletecorruptpacks)
    else:
        return historypackstore(ui, path, deletecorruptpacks)


class memhistorypack(object):
    def __init__(self):
        self.history = {}

    def add(self, name, node, p1, p2, linknode, copyfrom):
        self.history.setdefault(name, {})[node] = (p1, p2, linknode, copyfrom)

    def getmissing(self, keys):
        missing = []
        for name, node in keys:
            filehistory = self.history.get(name)
            if filehistory is None:
                missing.append((name, node))
            else:
                if node not in filehistory:
                    missing.append((name, node))
        return missing

    def getancestors(self, name, node, known=None):
        ancestors = {}
        try:
            ancestors[node] = self.history[name][node]
        except KeyError:
            raise KeyError((name, node))
        return ancestors

    def getnodeinfo(self, name, node):
        try:
            return self.history[name][node]
        except KeyError:
            raise KeyError((name, node))
