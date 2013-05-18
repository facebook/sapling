# remoterevlog.py - revlog implementation where the content is stored remotely
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import fileserverclient
import collections, os
from mercurial.node import bin, hex, nullid, nullrev
from mercurial import revlog, mdiff, bundle, filelog

def _readfile(path):
    f = open(path, "r")
    try:
        return f.read()
    finally:
        f.close()

def _writefile(path, content):
    dirname = os.path.dirname(path)
    if not os.path.exists(dirname):
        os.makedirs(dirname)

    f = open(path, "w")
    try:
        f.write(content)
    finally:
        f.close()

def _createrevlogtext(text, copyfrom=None, copyrev=None):
    """returns a string that matches the revlog contents in a
    traditional revlog
    """
    meta = {}
    if copyfrom or text.startswith('\1\n'):
        if copyfrom:
            meta['copy'] = copyfrom
            meta['copyrev'] = copyrev
        text = "\1\n%s\1\n%s" % (filelog._packmeta(meta), text)

    return text

class remotefilelog(object):
    def __init__(self, opener, path):
        self.opener = opener
        self.filename = path
        self.localpath = os.path.join(opener.vfs.base, 'localdata')

        if not os.path.exists(self.localpath):
            os.makedirs(self.localpath)

    def read(self, node):
        """returns the file contents at this node"""

        # the file blobs are formated as such:
        # blob => size of content + \0 + content + list(ancestors)
        # ancestor => node + p1 + p2 + linknode + copypath + \0

        raw = self._read(hex(node))
        index = raw.index('\0')
        size = int(raw[:index])

        return raw[(index + 1):(index + 1 + size)]

    def add(self, text, meta, transaction, linknode, p1=None, p2=None):
        hashtext = text

        # hash with the metadata, like in vanilla filelogs
        hashtext = _createrevlogtext(text, meta.get('copy'), meta.get('copyrev'))
        node = revlog.hash(hashtext, p1, p2)

        def _createfileblob():
            data = "%s\0%s" % (len(text), text)

            realp1 = p1
            copyfrom = ""
            if 'copy' in meta:
                copyfrom = meta['copy']
                realp1 = bin(meta['copyrev'])

            data += "%s%s%s%s%s\0" % (node, realp1, p2, linknode, copyfrom)

            pancestors = {}
            queue = []
            if realp1 != nullid:
                p1flog = self
                if copyfrom:
                    p1flog = remotefilelog(self.opener, copyfrom)

                pancestors.update(p1flog.ancestormap(realp1))
                queue.append(realp1)
            if p2 != nullid:
                pancestors.update(self.ancestormap(p2))
                queue.append(p2)

            visited = set()
            ancestortext = ""

            # add the ancestors in topological order
            while queue:
                c = queue.pop(0)
                visited.add(c)
                pa1, pa2, ancestorlinknode, pacopyfrom = pancestors[c]

                ancestortext += "%s%s%s%s%s\0" % (
                    c, pa1, pa2, ancestorlinknode, pacopyfrom)

                if pa1 != nullid and pa1 not in visited:
                    queue.append(pa1)
                if pa2 != nullid and pa2 not in visited:
                    queue.append(pa2)

            data += ancestortext

            return data

        key = fileserverclient.getcachekey(self.filename, hex(node))
        path = os.path.join(self.localpath, key)
        _writefile(path, _createfileblob())

        return node

    def renamed(self, node):
        raw = self._read(hex(node))
        index = raw.index('\0')
        size = int(raw[:index])

        offset = index + 1 + size
        p1 = raw[(offset + 20):(offset + 40)]
        copyoffset = offset + 80
        copyfromend = raw.index('\0', copyoffset)
        copyfrom = raw[copyoffset:copyfromend]

        if copyfrom:
            return (copyfrom, p1)

        return False

    def size(self, node):
        """return the size of a given revision"""

        raw = self._read(hex(node))
        index = raw.index('\0')
        size = int(raw[:index])
        return size

    def cmp(self, node, text):
        """compare text with a given file revision

        returns True if text is different than what is stored.
        """

        if node == nullid:
            return True

        nodetext = self.read(node)
        return nodetext != text

    def __len__(self):
        # hack
        if self.filename == '.hgtags':
            return 0

        raise Exception("len not supported")

    def parents(self, node):
        if node == nullid:
            return nullid, nullid

        ancestormap = self.ancestormap(node)
        p1, p2, linknode, copyfrom = ancestormap[node]
        if not copyfrom:
            p1 = nullid

        return p1, p2

    def linknode(self, node):
        raw = self._read(hex(node))
        index = raw.index('\0')
        size = int(raw[:index])
        offset = index + 1 + size + 60
        return raw[offset:(offset + 20)]

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
        """returns the revlog contents at this node.
        this includes the meta data traditionally included in file revlogs.
        this is generally only used for bundling and communicating with vanilla
        hg clients.
        """
        if node == nullid:
            return ""
        if len(node) != 20:
            raise LookupError(node, self.filename, _('invalid revision input'))

        raw = self._read(hex(node))

        index = raw.index('\0')
        size = int(raw[:index])
        data = raw[(index + 1):(index + 1 + size)]

        mapping = self.ancestormap(node)
        copyrev = None
        copyfrom = mapping[node][2]
        if copyfrom:
            copyrev = mapping[node][1]

        return _createrevlogtext(data, copyfrom, copyrev)

    def _read(self, id):
        """reads the raw file blob from disk, cache, or server"""
        key = fileserverclient.getcachekey(self.filename, id)
        cachepath = os.path.join(fileserverclient.client.cachepath, key)
        if os.path.exists(cachepath):
            return _readfile(cachepath)

        localpath = os.path.join(self.localpath, key)
        if os.path.exists(localpath):
            return _readfile(localpath)

        fileserverclient.client.prefetch(self.opener.vfs.base, [(self.filename, id)])
        if os.path.exists(cachepath):
            return _readfile(cachepath)

        raise LookupError(id, self.filename, _('no node'))

    def ancestormap(self, node):
        raw = self._read(hex(node))
        index = raw.index('\0')
        size = int(raw[:index])
        start = index + 1 + size

        mapping = {}
        while start < len(raw):
            divider = raw.index('\0', start + 80)

            node = raw[start:(start + 20)]
            p1 = raw[(start + 20):(start + 40)]
            p2 = raw[(start + 40):(start + 60)]
            linknode = raw[(start + 60):(start + 80)]
            copyfrom = raw[(start + 80):divider]

            mapping[node] = (p1, p2, linknode, copyfrom)
            start = divider + 1

        return mapping

    def strip(self, minlink, transaction):
        pass

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

    def group(self, nodelist, bundler, reorder=None):
        if len(nodelist) == 0:
            yield bundler.close()
            return

        nodelist = self._sortnodes(nodelist)

        # add the parent of the first rev
        p = self.parents(nodelist[0])[0]
        nodelist.insert(0, p)

        # build deltas
        for i in xrange(len(nodelist) - 1):
            prev, curr = nodelist[i], nodelist[i + 1]
            for c in bundler.nodechunk(self, curr, prev):
                yield c

        yield bundler.close()

    def _sortnodes(self, nodelist):
        """returns the topologically sorted nodes
        """
        if len(nodelist) == 1:
            return list(nodelist)

        nodes = set(nodelist)
        parents = {}
        allparents = set()
        for n in nodes:
            parents[n] = self.parents(n)
            allparents.update(parents[n])

        allparents.intersection_update(nodes)

        result = list()
        while nodes:
            root = None
            for n in nodes:
                p1, p2 = parents[n]
                if not p1 in allparents and not p2 in allparents:
                    root = n
                    break

            allparents.discard(root)
            result.append(root)
            nodes.remove(root)

        return result
