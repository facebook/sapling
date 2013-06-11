# remotefilelog.py - revlog implementation where the content is stored remotely
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import fileserverclient
import collections, os, shutil
from mercurial.node import bin, hex, nullid, nullrev
from mercurial import revlog, mdiff, filelog, ancestor, error
from mercurial.i18n import _

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

def _parsemeta(text):
    meta, keys, size = filelog._parsemeta(text)
    if text.startswith('\1\n'):
        s = text.index('\1\n', 2)
        text = text[s + 2:]
    return meta or {}, text

class remotefilelog(object):
    def __init__(self, opener, path, repo):
        self.opener = opener
        self.filename = path
        self.repo = repo
        self.localpath = os.path.join(opener.vfs.base, 'data')

        if not os.path.exists(self.localpath):
            os.makedirs(self.localpath)

        self.version = 1

    def read(self, node):
        """returns the file contents at this node"""

        if node == nullid:
            return ""

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

        key = fileserverclient.getcachekey(self.filename, hex(node))
        path = os.path.join(self.localpath, key)

        # if this node already exists, save the old version in case
        # we ever delete this new commit in the future
        if os.path.exists(path):
            filename = os.path.basename(path)
            directory = os.path.dirname(path)
            files = [f for f in os.listdir(directory) if f.startswith(filename)]
            shutil.copyfile(path, path + str(len(files)))

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

    def __nonzero__(self):
        if self.filename == '.hgtags':
            return False

        return True

    def __len__(self):
        # hack
        if self.filename == '.hgtags':
            return 0

        raise Exception("len not supported")

    def empty(self):
        return False

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
            raise error.LookupError(node, self.filename, _('invalid lookup input'))

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

        raw = self._read(hex(node))

        index = raw.index('\0')
        size = int(raw[:index])
        data = raw[(index + 1):(index + 1 + size)]

        mapping = self.ancestormap(node)
        p1, p2, linknode, copyfrom = mapping[node]
        copyrev = None
        if copyfrom:
            copyrev = hex(p1)

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

        raise error.LookupError(id, self.filename, _('no node'))

    def ancestormap(self, node):
        files = None
        while True:
            mapping = self._ancestormap(node)

            p1, p2, linknode, copyfrom = mapping[node]
            if linknode in self.repo:
                break
            else:
                if files == None:
                    # find a valid linknode by looking through the past versions
                    key = fileserverclient.getcachekey(self.filename, hex(node))
                    file = os.path.join(self.localpath, key)
                    filename = os.path.basename(file)
                    directory = os.path.dirname(file)
                    files = [f for f in os.listdir(directory) if f.startswith(filename)]
                    files = sorted(files, key=lambda x: int(x[40:] or '0'))

                    # store the current version in history, since we'll be
                    # replacing it
                    shutil.copyfile(file, file + str(len(files)))

                if len(files) == 1:
                    # Nothing to fallback to, just delete the local commit.
                    # The next loop will try the server, and throw an exception
                    # if the node still can't be found.
                    os.delete(file)
                else:
                    # Try the next version in the history
                    latest = files.pop()
                    shutil.copyfile(os.path.join(directory, latest), file)

        return mapping

    def _ancestormap(self, node):
        raw = self._read(hex(node))
        index = raw.index('\0')
        size = int(raw[:index])
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

        amap = self.ancestormap(a)
        bmap = self.ancestormap(b)

        def parents(x):
            p = amap.get(x)
            if not p:
                p = bmap.get(x)
            if p:
                return [p[0], p[1]]
            return []

        result = ancestor.genericancestor(a, b, parents)
        return result or nullid

    def strip(self, minlink, transaction):
        pass

    # misc unused things
    def files(self):
        return []

    def checksize(self):
        return 0, 0
