# remotefilelog.py - filelog implementation where filelog history is stored remotely
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import fileserverclient, ioutil
import collections, errno, os, shutil
from mercurial.node import bin, hex, nullid, nullrev
from mercurial import revlog, mdiff, filelog, ancestor, error, util
from mercurial.i18n import _

class remotefilelog(object):
    def __init__(self, opener, path, repo):
        self.opener = opener
        self.filename = path
        self.repo = repo
        self.localpath = os.path.join(opener.vfs.base, 'data')

        self.version = 1

    def read(self, node):
        t = self.revision(node)
        if not t.startswith('\1\n'):
            return t
        s = t.index('\1\n', 2)
        return t[s + 2:]

    def add(self, text, meta, transaction, linknode, p1=None, p2=None):
        hashtext = text

        # hash with the metadata, like in vanilla filelogs
        hashtext = ioutil.createrevlogtext(text, meta.get('copy'), meta.get('copyrev'))
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
                    p1flog = remotefilelog(self.opener, copyfrom, self.repo)

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

        data = _createfileblob()
        self.repo.contentstore.addremotefilelog(self.filename, node, data)

        return node

    def renamed(self, node):
        ancestors = self.repo.metadatastore.getancestors(self.filename, node)
        p1, p2, linknode, copyfrom = ancestors[node]
        if copyfrom:
            return (copyfrom, p1)

        return False

    def size(self, node):
        """return the size of a given revision"""
        content = self.repo.contentstore.get(self.filename, node)
        return len(content)

    rawsize = size

    def cmp(self, node, text):
        """compare text with a given file revision

        returns True if text is different than what is stored.
        """

        if node == nullid:
            return True

        nodetext = self.read(node)
        return nodetext != text

    def __nonzero__(self):
        return True

    def __len__(self):
        if self.filename == '.hgtags':
            # The length of .hgtags is used to fast path tag checking.
            # remotefilelog doesn't support .hgtags since the entire .hgtags
            # history is needed.  Use the excludepattern setting to make
            # .hgtags a normal filelog.
            return 0

        raise Exception("len not supported")

    def empty(self):
        return False

    def flags(self, rev):
        return revlog.REVIDX_DEFAULT_FLAGS

    def parents(self, node):
        if node == nullid:
            return nullid, nullid

        ancestormap = self.ancestormap(node)
        p1, p2, linknode, copyfrom = ancestormap[node]
        if copyfrom:
            p1 = nullid

        return p1, p2

    def linknode(self, node):
        raw = self._read(hex(node))
        index, size = ioutil.parsesize(raw)
        offset = index + 1 + size + 60
        return raw[offset:(offset + 20)]

    def revdiff(self, node1, node2):
        return mdiff.textdiff(self.revision(node1),
                              self.revision(node2))

    def lookup(self, node):
        if len(node) == 40:
            node = bin(node)
        if len(node) != 20:
            raise error.LookupError(node, self.filename, _('invalid lookup input'))

        return node

    def rev(self, node):
        # This is a hack to make TortoiseHG work.
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
            raise error.LookupError(node, self.filename, _('invalid revision input'))

        return self.repo.contentstore.get(self.filename, node)

    def _read(self, id):
        """reads the raw file blob from disk, cache, or server"""
        fileservice = self.repo.fileservice
        localcache = fileservice.localcache
        cachekey = fileserverclient.getcachekey(self.repo.name, self.filename, id)
        try:
            return localcache.read(cachekey)
        except KeyError:
            pass

        localkey = fileserverclient.getlocalkey(self.filename, id)
        localpath = os.path.join(self.localpath, localkey)
        try:
            return ioutil.readfile(localpath)
        except IOError:
            pass

        fileservice.prefetch([(self.filename, id)])
        try:
            return localcache.read(cachekey)
        except KeyError:
            pass

        raise error.LookupError(id, self.filename, _('no node'))

    def ancestormap(self, node):
        hexnode = hex(node)

        localcache = self.repo.fileservice.localcache
        reponame = self.repo.name

        # Check the local cache of remote data
        cachekey = fileserverclient.getcachekey(reponame, self.filename, hexnode)
        try:
            raw = localcache.read(cachekey)
            mapping = self._ancestormap(node, raw)
            if mapping:
                return mapping
        except KeyError:
            pass

        # Check our local commit data
        localkey = fileserverclient.getlocalkey(self.filename, hexnode)
        localpath = os.path.join(self.localpath, localkey)
        try:
            raw = ioutil.readfile(localpath)
            mapping = self._ancestormap(node, raw)
            if mapping:
                return mapping
        except IOError:
            pass

        # Fallback to the server cache
        self.repo.fileservice.prefetch([(self.filename, hexnode)],
            force=True)
        try:
            raw = localcache.read(cachekey)
            mapping = self._ancestormap(node, raw)
            if mapping:
                return mapping
        except KeyError:
            pass

        raise error.LookupError(node, self.filename, _('no valid file history'))

    def _ancestormap(self, node, raw):
        index, size = ioutil.parsesize(raw)
        start = index + 1 + size

        mapping = {}
        while start < len(raw):
            divider = raw.index('\0', start + 80)

            currentnode = raw[start:(start + 20)]
            p1 = raw[(start + 20):(start + 40)]
            p2 = raw[(start + 40):(start + 60)]
            linknode = raw[(start + 60):(start + 80)]
            copyfrom = raw[(start + 80):divider]

            mapping[currentnode] = (p1, p2, linknode, copyfrom)
            start = divider + 1

        return mapping

    def ancestor(self, a, b):
        if a == nullid or b == nullid:
            return nullid

        revmap, parentfunc = self._buildrevgraph(a, b)
        nodemap = dict(((v,k) for (k,v) in revmap.iteritems()))

        ancs = ancestor.ancestors(parentfunc, revmap[a], revmap[b])
        if ancs:
            # choose a consistent winner when there's a tie
            return min(map(nodemap.__getitem__, ancs))
        return nullid

    def commonancestorsheads(self, a, b):
        """calculate all the heads of the common ancestors of nodes a and b"""

        if a == nullid or b == nullid:
            return nullid

        revmap, parentfunc = self._buildrevgraph(a, b)
        nodemap = dict(((v,k) for (k,v) in revmap.iteritems()))

        ancs = ancestor.commonancestorsheads(parentfunc, revmap[a], revmap[b])
        return map(nodemap.__getitem__, ancs)

    def _buildrevgraph(self, a, b):
        """Builds a numeric revision graph for the given two nodes.
        Returns a node->rev map and a rev->[revs] parent function.
        """
        amap = self.ancestormap(a)
        bmap = self.ancestormap(b)

        # Union the two maps
        parentsmap = collections.defaultdict(list)
        allparents = set()
        for mapping in (amap, bmap):
            for node, pdata in mapping.iteritems():
                parents = parentsmap[node]
                p1, p2, linknode, copyfrom = pdata
                # Don't follow renames (copyfrom).
                # remotefilectx.ancestor does that.
                if p1 != nullid and not copyfrom:
                    parents.append(p1)
                    allparents.add(p1)
                if p2 != nullid:
                    parents.append(p2)
                    allparents.add(p2)


        # Breadth first traversal to build linkrev graph
        parentrevs = collections.defaultdict(list)
        revmap = {}
        queue = collections.deque(((None, n) for n in parentsmap.iterkeys()
                 if n not in allparents))
        while queue:
            prevrev, current = queue.pop()
            if current in revmap:
                if prevrev:
                    parentrevs[prevrev].append(revmap[current])
                continue

            # Assign linkrevs in reverse order, so start at
            # len(parentsmap) and work backwards.
            currentrev = len(parentsmap) - len(revmap) - 1
            revmap[current] = currentrev

            if prevrev:
                parentrevs[prevrev].append(currentrev)

            for parent in parentsmap.get(current):
                queue.appendleft((currentrev, parent))

        return revmap, parentrevs.__getitem__

    def strip(self, minlink, transaction):
        pass

    # misc unused things
    def files(self):
        return []

    def checksize(self):
        return 0, 0
