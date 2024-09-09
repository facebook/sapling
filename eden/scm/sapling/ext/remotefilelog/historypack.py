# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import struct

# (filename hash, offset, size)
INDEXFORMAT0 = "!20sQQ"
INDEXENTRYLENGTH0: int = struct.calcsize(INDEXFORMAT0)
INDEXFORMAT1 = "!20sQQII"
INDEXENTRYLENGTH1: int = struct.calcsize(INDEXFORMAT1)
NODELENGTH = 20

NODEINDEXFORMAT = "!20sQ"
NODEINDEXENTRYLENGTH: int = struct.calcsize(NODEINDEXFORMAT)

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


class memhistorypack:
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

    def getnodeinfo(self, name, node):
        try:
            return self.history[name][node]
        except KeyError:
            raise KeyError((name, node))
