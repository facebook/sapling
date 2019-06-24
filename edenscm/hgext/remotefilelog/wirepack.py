# wirepack.py - wireprotocol for exchanging packs
#
# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
from __future__ import absolute_import

import struct
from collections import defaultdict
from StringIO import StringIO

from edenscm.mercurial import perftrace, progress
from edenscm.mercurial.i18n import _
from edenscm.mercurial.node import hex, nullid

from . import constants, shallowutil
from .shallowutil import buildpackmeta, parsepackmeta, readexactly, readpath, readunpack


try:
    xrange(0)
except NameError:
    xrange = range


def sendpackpart(filename, history, data, version=1):
    """A wirepack is formatted as follows:

    wirepack = <filename len: 2 byte unsigned int><filename>
               <history len: 4 byte unsigned int>[<history rev>,...]
               <data len: 4 byte unsigned int>[<data rev>,...]

    hist rev = <node: 20 byte>
               <p1node: 20 byte>
               <p2node: 20 byte>
               <linknode: 20 byte>
               <copyfromlen: 2 byte unsigned int>
               <copyfrom>

    data rev = <node: 20 byte>
               <deltabasenode: 20 byte>
               <delta len: 8 byte unsigned int>
               <delta>
    """
    rawfilenamelen = struct.pack(constants.FILENAMESTRUCT, len(filename))
    yield "%s%s" % (rawfilenamelen, filename)

    # Serialize and send history
    historylen = struct.pack("!I", len(history))
    rawhistory = ""
    for entry in history:
        copyfrom = entry[4] or ""
        copyfromlen = len(copyfrom)
        tup = entry[:-1] + (copyfromlen,)
        rawhistory += struct.pack("!20s20s20s20sH", *tup)
        if copyfrom:
            rawhistory += copyfrom

    yield "%s%s" % (historylen, rawhistory)

    # Serialize and send data
    yield struct.pack("!I", len(data))

    for node, deltabase, delta, revlogflags in data:
        deltalen = struct.pack("!Q", len(delta))
        if version == 1:
            yield "%s%s%s%s" % (node, deltabase, deltalen, delta)
        elif version == 2:
            assert deltabase == nullid
            rawdata = "%s%s%s%s" % (node, deltabase, deltalen, delta)
            metadata = {
                constants.METAKEYFLAG: revlogflags,
                constants.METAKEYSIZE: len(delta),
            }
            metadata = buildpackmeta(metadata)
            rawdata += struct.pack("!I", len(metadata)) + metadata
            yield rawdata
        else:
            raise RuntimeError("Unsupported version %d", version)


def closepart():
    return "\0" * 10


def receivepack(ui, fh, dpack, hpack, version=1):
    receiveddata = []
    receivedhistory = []

    size = 0
    with progress.bar(ui, _("receiving pack")) as prog:
        while True:
            filename = readpath(fh)
            count = 0

            # Store the history for later sorting
            for value in readhistory(fh):
                node, p1, p2, linknode, copyfrom = value
                hpack.add(filename, node, p1, p2, linknode, copyfrom)
                receivedhistory.append((filename, node))
                count += 1
                size += len(filename) + len(node) + sum(len(x or "") for x in value)

            for node, deltabase, delta, metadata in readdeltas(fh, version=version):
                dpack.add(filename, node, deltabase, delta, metadata=metadata)
                receiveddata.append((filename, node))
                count += 1
                size += len(filename) + len(node) + len(deltabase) + len(delta)

            if count == 0 and filename == "":
                break
            prog.value += 1
    perftrace.tracebytes("Received Pack Size", size)

    return receiveddata, receivedhistory


def readhistory(fh):
    count = readunpack(fh, "!I")[0]
    for i in xrange(count):
        entry = readunpack(fh, "!20s20s20s20sH")
        if entry[4] != 0:
            copyfrom = readexactly(fh, entry[4])
        else:
            copyfrom = ""
        entry = entry[:4] + (copyfrom,)
        yield entry


def readdeltas(fh, version=1):
    count = readunpack(fh, "!I")[0]
    for i in xrange(count):
        node, deltabase, deltalen = readunpack(fh, "!20s20sQ")
        delta = readexactly(fh, deltalen)
        if version == 1:
            yield (node, deltabase, delta, None)
        elif version == 2:
            metalen, = readunpack(fh, "!I")
            meta = readexactly(fh, metalen)
            metadata = parsepackmeta(meta)
            yield (node, deltabase, delta, metadata)


class wirepackstore(object):
    def __init__(self, wirepack, version=1):
        self._data = {}
        self._history = {}
        fh = StringIO(wirepack)
        self._load(fh, version)

    def __iter__(self):
        for key in self._data:
            yield key

    def get(self, name, node):
        raise RuntimeError("must use getdeltachain with wirepackstore")

    def getdeltachain(self, name, node):
        delta, deltabase, metadata = self._data[(name, node)]
        return [(name, node, name, deltabase, delta)]

    def getmeta(self, name, node):
        try:
            delta, deltabase, metadata = self._data[(name, node)]
            if metadata is not None:
                return metadata
        except KeyError:
            raise KeyError((name, hex(node)))
        return {constants.METAKEYFLAG: "", constants.METAKEYSIZE: len(delta)}

    def getancestors(self, name, node, known=None):
        if known is None:
            known = set()
        if node in known:
            return []

        ancestors = {}
        seen = set()
        missing = [(name, node)]
        while missing:
            curname, curnode = missing.pop()
            info = self._history.get((name, node))
            if info is None:
                continue

            p1, p2, linknode, copyfrom = info
            if p1 != nullid and p1 not in known:
                key = (name if not copyfrom else copyfrom, p1)
                if key not in seen:
                    seen.add(key)
                    missing.append(key)
            if p2 != nullid and p2 not in known:
                key = (name, p2)
                if key not in seen:
                    seen.add(key)
                    missing.append(key)

            ancestors[curnode] = (p1, p2, linknode, copyfrom)
        if not ancestors:
            raise KeyError((name, hex(node)))
        return ancestors

    def getnodeinfo(self, name, node):
        try:
            return self._history[(name, node)]
        except KeyError:
            raise KeyError((name, hex(node)))

    def add(self, *args):
        raise RuntimeError("cannot add to a wirepack store")

    def getmissing(self, keys):
        missing = []
        for name, node in keys:
            if (name, node) not in self._data:
                missing.append((name, node))

        return missing

    def _load(self, fh, version):
        data = self._data
        history = self._history
        while True:
            filename = readpath(fh)
            count = 0

            # Store the history for later sorting
            for value in readhistory(fh):
                node = value[0]
                history[(filename, node)] = value[1:]
                count += 1

            for node, deltabase, delta, metadata in readdeltas(fh, version=version):
                data[(filename, node)] = (delta, deltabase, metadata)
                count += 1

            if count == 0 and filename == "":
                break

    def markledger(self, ledger, options=None):
        pass

    def cleanup(self, ledger):
        pass

    def debugstats(self):
        return "%d data items, %d history items" % (len(self._data), len(self._history))
