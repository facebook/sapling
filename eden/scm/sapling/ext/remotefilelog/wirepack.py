# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# wirepack.py - wireprotocol for exchanging packs


import io
import struct
import time
from typing import (
    cast,
    Dict,
    Generator,
    IO,
    Iterable,
    List,
    Optional,
    Sequence,
    Tuple,
    TYPE_CHECKING,
)

from sapling import perftrace, progress
from sapling.i18n import _
from sapling.node import hex, nullid

from . import constants
from .mutablestores import mutabledatastore, mutablehistorystore
from .shallowutil import buildpackmeta, parsepackmeta, readexactly, readpath, readunpack

if TYPE_CHECKING:
    import sapling


def sendpackpart(
    filename: str,
    history: "Sequence[Tuple[bytes, bytes, bytes, bytes, Optional[str]]]",
    data: "Sequence[Tuple[bytes, bytes, bytes, int]]",
    version: int = 1,
) -> "Iterable[bytes]":
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
    rawfilename = filename.encode()
    rawfilenamelen = struct.pack(constants.FILENAMESTRUCT, len(rawfilename))
    yield b"%s%s" % (rawfilenamelen, rawfilename)

    # Serialize and send history
    historylen = struct.pack("!I", len(history))
    rawhistory = []
    for entry in history:
        copyfrom = (entry[4] or "").encode()
        copyfromlen = len(copyfrom)
        tup = entry[:-1] + (copyfromlen,)
        rawhistory.append(struct.pack("!20s20s20s20sH", *tup))
        if copyfrom:
            rawhistory.append(copyfrom)

    rawhistory = b"".join(rawhistory)

    yield b"%s%s" % (historylen, rawhistory)

    # Serialize and send data
    yield struct.pack("!I", len(data))

    for node, deltabase, delta, revlogflags in data:
        deltalen = struct.pack("!Q", len(delta))
        if version == 1:
            yield b"%s%s%s%s" % (node, deltabase, deltalen, delta)
        elif version == 2:
            assert deltabase == nullid
            rawdata = b"%s%s%s%s" % (node, deltabase, deltalen, delta)
            metadata = {
                constants.METAKEYFLAG: revlogflags,
                constants.METAKEYSIZE: len(delta),
            }
            metadata = buildpackmeta(metadata)
            rawdata += struct.pack("!I", len(metadata)) + metadata
            yield rawdata
        else:
            raise RuntimeError("Unsupported version %d", version)


def closepart() -> bytes:
    return b"\0" * 10


def receivepack(
    ui: "sapling.ui.ui",
    fh: "IO[bytes]",
    dpack: "mutabledatastore",
    hpack: "mutablehistorystore",
    version: int = 1,
) -> "Tuple[List[Tuple[bytes, bytes]], List[Tuple[bytes, bytes]]]":
    receiveddata = []
    receivedhistory = []

    size = 0
    start = time.time()
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
    duration = time.time() - start
    megabytes = float(size) / 1024 / 1024
    if ui.configbool("remotefilelog", "debug-fetches") and (
        duration > 1 or len(receiveddata) > 100 or megabytes > 1
    ):
        ui.warn(
            _("Receive pack: %s entries, %.2f MB, %.2f seconds (%0.2f MBps)\n")
            % (len(receiveddata), megabytes, duration, megabytes / duration)
        )

    return receiveddata, receivedhistory


def readhistory(
    fh: "IO[bytes]",
) -> "Generator[Tuple[bytes, bytes, bytes, bytes, bytes], None, None]":
    count = readunpack(fh, "!I")[0]
    for i in range(count):
        entry = readunpack(fh, "!20s20s20s20sH")
        if entry[4] != 0:
            copyfrom = readexactly(fh, entry[4]).decode()
        else:
            copyfrom = ""
        entry = entry[:4] + (copyfrom,)
        yield cast("Tuple[bytes, bytes, bytes, bytes, bytes]", entry)


def readdeltas(
    fh: "IO[bytes]", version: int = 1
) -> "Generator[Tuple[bytes, bytes, bytes, Optional[Dict[str, int]]], None, None]":
    count = readunpack(fh, "!I")[0]
    for i in range(count):
        node, deltabase, deltalen = readunpack(fh, "!20s20sQ")
        assert isinstance(node, bytes) and isinstance(deltabase, bytes)
        delta = readexactly(fh, deltalen)
        if version == 1:
            yield (node, deltabase, delta, None)
        elif version == 2:
            (metalen,) = readunpack(fh, "!I")
            meta = readexactly(fh, metalen)
            metadata = parsepackmeta(meta)
            yield (node, deltabase, delta, metadata)


class wirepackstore:
    def __init__(self, wirepack, version=1):
        self._data = {}
        self._history = {}
        fh = io.BytesIO(wirepack)
        self._load(fh, version)

    def __iter__(self):
        for key in self._data:
            yield key

    def get(self, name, node):
        raise RuntimeError("must use getdeltachain with wirepackstore")

    def getdeltachain(self, name, node):
        delta, deltabase, metadata = self._data[(name, node)]
        return [(name, node, name, deltabase, delta)]

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

    def cleanup(self, ledger):
        pass

    def debugstats(self):
        return "%d data items, %d history items" % (len(self._data), len(self._history))
