# wirepack.py - wireprotocol for exchanging packs
#
# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial.i18n import _
from mercurial.node import nullid
import constants
import struct
from collections import defaultdict

from shallowutil import readexactly, readunpack, mkstickygroupdir, readpath
import datapack, historypack, shallowutil

def sendpackpart(filename, history, data):
    rawfilenamelen = struct.pack(constants.FILENAMESTRUCT,
                                 len(filename))
    yield '%s%s' % (rawfilenamelen, filename)

    # Serialize and send history
    historylen = struct.pack('!I', len(history))
    rawhistory = ''
    for entry in history:
        copyfrom = entry[4] or ''
        copyfromlen = len(copyfrom)
        tup = entry[:-1] + (copyfromlen,)
        rawhistory += struct.pack('!20s20s20s20sH', *tup)
        if copyfrom:
            rawhistory += copyfrom

    yield '%s%s' % (historylen, rawhistory)

    # Serialize and send data
    yield struct.pack('!I', len(data))

    for node, deltabase, delta in data:
        deltalen = struct.pack('!Q', len(delta))
        yield '%s%s%s%s' % (node, deltabase, deltalen, delta)

def closepart():
    return '\0' * 10

def receivepack(ui, fh, packpath):
    receiveddata = []
    receivedhistory = []
    mkstickygroupdir(ui, packpath)
    totalcount = 0
    ui.progress(_("receiving pack"), totalcount)
    with datapack.mutabledatapack(ui, packpath) as dpack:
        with historypack.mutablehistorypack(ui, packpath) as hpack:
            pendinghistory = defaultdict(dict)
            while True:
                filename = readpath(fh)
                count = 0

                # Store the history for later sorting
                for value in readhistory(fh):
                    node = value[0]
                    pendinghistory[filename][node] = value
                    receivedhistory.append((filename, node))
                    count += 1

                for node, deltabase, delta in readdeltas(fh):
                    dpack.add(filename, node, deltabase, delta)
                    receiveddata.append((filename, node))
                    count += 1

                if count == 0 and filename == '':
                    break
                totalcount += 1
                ui.progress(_("receiving pack"), totalcount)

            # Add history to pack in toposorted order
            for filename, nodevalues in sorted(pendinghistory.iteritems()):
                def _parentfunc(node):
                    p1, p2 = nodevalues[node][1:3]
                    parents = []
                    if p1 != nullid:
                        parents.append(p1)
                    if p2 != nullid:
                        parents.append(p2)
                    return parents
                sortednodes = reversed(shallowutil.sortnodes(
                                        nodevalues.iterkeys(),
                                        _parentfunc))
                for node in sortednodes:
                    node, p1, p2, linknode, copyfrom = nodevalues[node]
                    hpack.add(filename, node, p1, p2, linknode, copyfrom)
    ui.progress(_("receiving pack"), None)

    return receiveddata, receivedhistory

def readhistory(fh):
    count = readunpack(fh, '!I')[0]
    for i in xrange(count):
        entry = readunpack(fh,'!20s20s20s20sH')
        if entry[4] != 0:
            copyfrom = readexactly(fh, entry[4])
        else:
            copyfrom = ''
        entry = entry[:4] + (copyfrom,)
        yield entry

def readdeltas(fh):
    count = readunpack(fh, '!I')[0]
    for i in xrange(count):
        node, deltabase, deltalen = readunpack(fh, '!20s20sQ')
        delta = readexactly(fh, deltalen)
        yield (node, deltabase, delta)
