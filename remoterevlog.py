# remoterevlog.py - revlog implementation where the content is stored remotely
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import fileserverclient
import collections, os
from mercurial.node import bin, hex, nullid, nullrev
from mercurial import revlog, mdiff, bundle

def _readfile(path):
    f = open(path, "r")
    try:
        return f.read()
    finally:
        f.close()

def _writefile(path, content):
    f = open(path, "w")
    try:
        f.write(content)
    finally:
        f.close()

class remoterevlog(object):
    """A partial implementation of the revlog api where the revision contents
    are stored remotely on a server. It differs from normal revlogs in that it
    doesn't have rev numbers.
    """
    def __init__(self, opener, path):
        self.opener = opener
        self.filename = path[5:-2]
        self.localpath = os.path.join(opener.vfs.base, 'localdata')

        if not os.path.exists(self.localpath):
            os.makedirs(self.localpath)

    def __len__(self):
        # hack
        if self.filename == '.hgtags':
            return 0

        raise Exception("len not supported")

    def parents(self, node):
        if node == nullid:
            return nullid, nullid
        raw = self._read(hex(node))
        return raw[:20], raw[20:40]

    def rawsize(self, node):
        return len(self.revision(node))
    size = rawsize

    def cmp(self, node, text):
        p1, p2 = self.parents(node)
        return revlog.hash(text, p1, p2) != node

    def revdiff(self, node1, node2):
        return mdiff.textdiff(self.revision(node1),
                              self.revision(node2))

    def lookup(self, node):
        if len(node) == 40:
            node = bin(node)
        if len(node) != 20:
            raise LookupError(node, self.filename, _('invalid lookup input'))

        return node

    def revision(self, node):
        if node == nullid:
            return ""
        if len(node) != 20:
            raise LookupError(node, self.filename, _('invalid revision input'))

        raw = self._read(hex(node))
        return raw[40:]

    def _read(self, id):
        cachepath = os.path.join(fileserverclient.client.cachepath, id)
        if os.path.exists(cachepath):
            return _readfile(cachepath)

        result = self._localread(id)
        if result != None:
            return result

        result = self._remoteread(id)

        if result == None:
            raise LookupError(id, self.filename, _('no node'))

        return result

    def _localread(self, id):
        localpath = os.path.join(self.localpath, id)
        if os.path.exists(localpath):
            return _readfile(localpath)

        return None

    def _remoteread(self, id):
        fileserverclient.prefetch(self.opener.vfs.base, [(self.filename, id)])

        cachepath = os.path.join(fileserverclient.client.cachepath, id)
        if os.path.exists(cachepath):
            return _readfile(cachepath)

        return None

    def strip(self, minlink, transaction):
        pass

    def addrevision(self, text, transaction, link, p1, p2, cachedelta=None):
        node = revlog.hash(text, p1, p2)

        path = os.path.join(self.localpath, hex(node))
        _writefile(path, p1 + p2 + text)

        return node

    def addgroup(self, bundle, linkmapper, transaction):
        chain = None
        while True:
            chunkdata = bundle.deltachunk(chain)
            if not chunkdata:
                break
            node = chunkdata['node']
            p1 = chunkdata['p1']
            p2 = chunkdata['p2']
            cs = chunkdata['cs']
            deltabase = chunkdata['deltabase']
            delta = chunkdata['delta']

            base = self.revision(deltabase)
            text = mdiff.patch(base, delta)
            if isinstance(text, buffer):
                text = str(text)

            link = linkmapper(cs)
            chain = self.addrevision(text, transaction, link, p1, p2)

        return True
