# remotefilelog.py - filelog implementation where filelog history is stored remotely
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
        result = f.read()

        # we should never have empty files
        if not result:
            os.remove(path)
            raise IOError("empty file: %s" % path)

        return result
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
        text = filelog.packmeta(meta, text)

    return text

def _parsemeta(text):
    meta, size = filelog.parsemeta(text)
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

        self.version = 1

    def read(self, node):
        """returns the file contents at this node"""

        if node == nullid:
            return ""

        # the file blobs are formated as such:
        # blob => size of content + \0 + content + list(ancestors)
        # ancestor => node + p1 + p2 + linknode + copypath + \0

        raw = self._read(hex(node))
        index, size = self._parsesize(raw)
        return raw[(index + 1):(index + 1 + size)]

    def _parsesize(self, raw):
        try:
            index = raw.index('\0')
            size = int(raw[:index])
        except ValueError:
            raise Exception("corrupt cache data for '%s'" % (self.filename))
        return index, size

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

                pancestors.update(p1flog.ancestormap(realp1, relativeto=linknode))
                queue.append(realp1)
            if p2 != nullid:
                pancestors.update(self.ancestormap(p2, relativeto=linknode))
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

        key = fileserverclient.getlocalkey(self.filename, hex(node))
        path = os.path.join(self.localpath, key)

        # if this node already exists, save the old version in case
        # we ever delete this new commit in the future
        oldumask = os.umask(0o002)
        try:
            if os.path.exists(path):
                filename = os.path.basename(path)
                directory = os.path.dirname(path)
                files = [f for f in os.listdir(directory) if f.startswith(filename)]
                shutil.copyfile(path, path + str(len(files)))

            _writefile(path, _createfileblob())
        finally:
            os.umask(oldumask)

        return node

    def renamed(self, node):
        raw = self._read(hex(node))
        index, size = self._parsesize(raw)

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
        index, size = self._parsesize(raw)
        return size

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
        index, size = self._parsesize(raw)
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

        index, size = self._parsesize(raw)
        data = raw[(index + 1):(index + 1 + size)]

        mapping = self.ancestormap(node)
        p1, p2, linknode, copyfrom = mapping[node]
        copyrev = None
        if copyfrom:
            copyrev = hex(p1)

        return _createrevlogtext(data, copyfrom, copyrev)

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
            return _readfile(localpath)
        except IOError:
            pass

        fileservice.prefetch([(self.filename, id)])
        try:
            return localcache.read(cachekey)
        except KeyError:
            pass

        raise error.LookupError(id, self.filename, _('no node'))

    def ancestormap(self, node, relativeto=None):
        # ancestormaps are a bit complex, and here's why:
        #
        # The key for filelog blobs contains the hash for the file path and for
        # the file version.  But these hashes do not include information about
        # the linknodes included in the blob. So it's possible to have multiple
        # blobs with the same key but different linknodes (for example, if you
        # rebase you will have the exact same file version, but with a different
        # linknode). So when reading the ancestormap (which contains linknodes)
        # we need to make sure all the linknodes are valid in this repo, so we
        # read through all versions that have ever existed, and pick one that
        # contains valid linknodes. If we can't find one locally, we then try
        # the server.

        hexnode = hex(node)

        localcache = self.repo.fileservice.localcache
        reponame = self.repo.name
        for i in range(0,2):
            cachekey = fileserverclient.getcachekey(reponame, self.filename, hexnode)
            try:
                raw = localcache.read(cachekey)
                mapping = self._ancestormap(node, raw, relativeto, fromserver=True)
                if mapping:
                    return mapping
            except KeyError:
                pass

            localkey = fileserverclient.getlocalkey(self.filename, hexnode)
            localpath = os.path.join(self.localpath, localkey)
            try:
                raw = _readfile(localpath)
                mapping = self._ancestormap(node, raw, relativeto)
                if mapping:
                    return mapping
            except IOError:
                pass

            # past versions may contain valid linknodes
            try:
                filename = os.path.basename(localpath)
                directory = os.path.dirname(localpath)
                alternates = [f for f in os.listdir(directory) if
                         len(f) > 40 and f.startswith(filename)]
                alternates = sorted(alternates, key=lambda x: int(x[40:]))

                for alternate in alternates:
                    alternatepath = os.path.join(directory, alternate)
                    try:
                        raw = _readfile(alternatepath)
                        mapping = self._ancestormap(node, raw, relativeto)
                        if mapping:
                            return mapping
                    except IOError:
                        pass
            except OSError:
                # Directory doesn't exist. Oh well
                pass

            # If exists locally, but with a bad history, adjust the linknodes
            # manually.
            if relativeto and os.path.exists(localpath):
                raw = _readfile(localpath)
                mapping = self._ancestormap(node, raw, relativeto,
                    adjustlinknodes=True)
                if mapping:
                    return mapping

            # Fallback to the server cache
            self.repo.fileservice.prefetch([(self.filename, hexnode)],
                force=True)
            try:
                raw = localcache.read(cachekey)
                mapping = self._ancestormap(node, raw, relativeto, fromserver=True)
                if mapping:
                    return mapping
            except KeyError:
                pass

        raise error.LookupError(node, self.filename, _('no valid file history'))

    def _ancestormap(self, node, raw, relativeto, fromserver=False,
            adjustlinknodes=False):
        index, size = self._parsesize(raw)
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

        # check that all linknodes are valid
        def validmap(node):
            queue = [node]

            repo = self.repo

            # When writing new file revisions, we need a ancestormap
            # that contains only linknodes that are ancestors of the new commit.
            # Otherwise it's possible that a linknode in the ancestormap might
            # be stripped, resulting in a permanently broken map.
            if relativeto:
                # If we're starting from a hidden node, allow hidden ancestors.
                if relativeto not in repo and relativeto in repo.unfiltered():
                    repo = repo.unfiltered()

                p1, p2, linknode, copyfrom = mapping[node]
                if not linknode in repo:
                    return False

                cl = repo.changelog
                common = cl.ancestor(linknode, relativeto)
                if common != linknode:
                    # Invalid key, unless it's from the server
                    return fromserver

            # Also check that the linknodes actually exist.
            while queue:
                node = queue.pop(0)
                p1, p2, linknode, copyfrom = mapping[node]
                if not linknode in repo:
                    return False
                if p1 != nullid:
                    queue.append(p1)
                if p2 != nullid:
                    queue.append(p2)

            return True

        if not validmap(node):
            if adjustlinknodes and relativeto:
                return self._fixmappinglinknodes(mapping, node, relativeto)
            return None

        return mapping

    def _fixmappinglinknodes(self, mapping, node, relativeto):
        """Takes a known-invalid mapping and does the minimal amount of work to
        produce a valid mapping, given the desired relative commit."""
        repo = self.repo
        cl = repo.unfiltered().changelog
        ma = repo.manifest

        newmapping = {}

        # Given a file node and a source commit, fills in the newmapping
        # with the valid history of that file node, relative to the source
        # commit.
        # Can't use recursion here since it might exceed the callstack depth.
        stack = [(self.filename, node, relativeto, False)]
        while stack:
            path, fnode, source, autoaccept = stack.pop()
            if fnode == nullid or fnode in newmapping:
                continue

            p1, p2, linknode, copyfrom = mapping[fnode]
            if (autoaccept or (linknode in cl.nodemap and
                cl.ancestor(linknode, source) == linknode)):
                newmapping[fnode] = p1, p2, linknode, copyfrom
                stack.append((path, p2, linknode, True))
                stack.append((copyfrom or path, p1, linknode, True))
            else:
                srcrev = cl.rev(source)
                iteranc = cl.ancestors([srcrev], inclusive=True)
                for a in iteranc:
                    ac = cl.read(a) # get changeset data (we avoid object creation)
                    if path in ac[3]: # checking the 'files' field.
                        # The file has been touched, check if the filenode is
                        # the same one we're searching for.
                        if fnode == ma.readfast(ac[0]).get(path):
                            linknode = cl.node(a)
                            newmapping[fnode] = p1, p2, linknode, copyfrom
                            stack.append((path, p2, linknode, False))
                            stack.append((copyfrom or path, p1, linknode, False))
                            break
                else:
                    # This shouldn't happen, but if for some reason we are unable to
                    # resolve a correct mapping, give up and let the _ancestormap()
                    # above try a new method.
                    msg = ("remotefilelog: unable to find valid history for "
                        "%s:%s" % (path, hex(fnode)))
                    repo.ui.warn(msg)
                    return None

        return newmapping

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
