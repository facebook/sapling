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

try:
    from edenscmnative import cstore

    cstore.datapack
except ImportError:
    cstore = None

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


class fastdatapack(basepack.basepack):
    INDEXSUFFIX = INDEXSUFFIX
    PACKSUFFIX = PACKSUFFIX

    def __init__(self, path):
        self._path = path
        self._packpath = path + self.PACKSUFFIX
        self._indexpath = path + self.INDEXSUFFIX
        self.datapack = cstore.datapack(path)

    def getmissing(self, keys):
        missing = []
        for name, node in keys:
            value = self.datapack._find(node)
            if not value:
                missing.append((name, node))

        return missing

    def get(self, name, node):
        raise RuntimeError(
            "must use getdeltachain with datapack (%s:%s)" % (name, hex(node))
        )

    def getmeta(self, name, node):
        return self.datapack.getmeta(node)

    def getdelta(self, name, node):
        result = self.datapack.getdelta(node)
        if result is None:
            raise KeyError((name, hex(node)))

        delta, deltabasenode, meta = result
        return delta, name, deltabasenode, meta

    def getdeltachain(self, name, node):
        result = self.datapack.getdeltachain(node)
        if result is None:
            raise KeyError((name, hex(node)))

        return result

    def add(self, name, node, data):
        raise RuntimeError("cannot add to datapack (%s:%s)" % (name, node))

    def markledger(self, ledger, options=None):
        if options and options.get(constants.OPTION_LOOSEONLY):
            return

        with ledger.location(self._path):
            for filename, node in self:
                ledger.markdataentry(self, filename, node)

    def cleanup(self, ledger):
        entries = ledger.sources.get(self, [])
        allkeys = set(self)
        repackedkeys = set(
            (e.filename, e.node) for e in entries if e.datarepacked or e.gced
        )

        if len(allkeys - repackedkeys) == 0:
            if self._path not in ledger.created:
                util.unlinkpath(self.indexpath(), ignoremissing=True)
                util.unlinkpath(self.packpath(), ignoremissing=True)

    def __iter__(self):
        return self.datapack.__iter__()

    def iterentries(self):
        return self.datapack.iterentries()


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
