# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.


NODELENGTH = 20

# The indicator value in the index for a fulltext entry.
FULLTEXTINDEXMARK = -1
NOBASEINDEXMARK = -2

INDEXSUFFIX = ".dataidx"
PACKSUFFIX = ".datapack"


class memdatapack:
    def __init__(self):
        self.data = {}

    def add(self, name, node, deltabase, delta):
        self.data[(name, node)] = (deltabase, delta)

    def getdelta(self, name, node):
        deltabase, delta = self.data[(name, node)]
        return (delta, name, deltabase, {})

    def getdeltachain(self, name, node):
        deltabase, delta = self.data[(name, node)]
        return [(name, node, name, deltabase, delta)]

    def getmissing(self, keys):
        missing = []
        for key in keys:
            if key not in self.data:
                missing.append(key)
        return missing
