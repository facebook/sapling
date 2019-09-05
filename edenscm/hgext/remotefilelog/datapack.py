# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import os
import struct

from edenscm.mercurial import error, util
from edenscm.mercurial.i18n import _
from edenscm.mercurial.node import hex, nullid
from edenscmnative.bindings import revisionstore

from . import basepack, constants, shallowutil
from .lz4wrapper import lz4compress, lz4decompress


try:
    xrange(0)
except NameError:
    xrange = range

NODELENGTH = 20

# The indicator value in the index for a fulltext entry.
FULLTEXTINDEXMARK = -1
NOBASEINDEXMARK = -2

INDEXSUFFIX = ".dataidx"
PACKSUFFIX = ".datapack"


class datapackstore(basepack.basepackstore):
    INDEXSUFFIX = INDEXSUFFIX
    PACKSUFFIX = PACKSUFFIX

    def __init__(self, ui, path, deletecorruptpacks=False):
        super(datapackstore, self).__init__(
            ui, path, deletecorruptpacks=deletecorruptpacks
        )

    def getpack(self, path):
        return revisionstore.datapack(path)

    def get(self, name, node):
        raise RuntimeError("must use getdeltachain with datapackstore")

    def getmeta(self, name, node):
        def func(pack):
            return pack.getmeta(name, node)

        for meta in self.runonpacks(func):
            return meta

        raise KeyError((name, hex(node)))

    def getdelta(self, name, node):
        def func(pack):
            return pack.getdelta(name, node)

        for delta in self.runonpacks(func):
            return delta

        raise KeyError((name, hex(node)))

    def getdeltachain(self, name, node):
        def func(pack):
            return pack.getdeltachain(name, node)

        for deltachain in self.runonpacks(func):
            return deltachain

        raise KeyError((name, hex(node)))

    def add(self, name, node, data):
        raise RuntimeError("cannot add to datapackstore")

    def repackstore(self, incremental=True):
        if self.fetchpacksenabled:
            revisionstore.repackincrementaldatapacks(self.path, self.path)


def makedatapackstore(ui, path, deletecorruptpacks=False):
    if ui.configbool("remotefilelog", "userustpackstore", False):
        return revisionstore.datapackstore(path, deletecorruptpacks)
    else:
        return datapackstore(ui, path, deletecorruptpacks)


class memdatapack(object):
    def __init__(self):
        self.data = {}
        self.meta = {}

    def add(self, name, node, deltabase, delta):
        self.data[(name, node)] = (deltabase, delta)

    def getdelta(self, name, node):
        deltabase, delta = self.data[(name, node)]
        return (delta, name, deltabase, self.getmeta(name, node))

    def getdeltachain(self, name, node):
        deltabase, delta = self.data[(name, node)]
        return [(name, node, name, deltabase, delta)]

    def getmeta(self, name, node):
        return self.meta[(name, node)]

    def getmissing(self, keys):
        missing = []
        for key in keys:
            if key not in self.data:
                missing.append(key)
        return missing
