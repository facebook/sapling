# remotefilelog.py - filelog implementation where filelog history is stored
#                    remotely
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
from __future__ import absolute_import

import collections
import os

from bindings import revisionstore
from edenscm.mercurial import ancestor, error, filelog, mdiff, revlog, util
from edenscm.mercurial.i18n import _
from edenscm.mercurial.node import bin, nullid

from . import constants, fileserverclient, mutablestores, shallowutil
from .contentstore import remotecontentstore, unioncontentstore
from .datapack import makedatapackstore
from .historypack import makehistorypackstore
from .metadatastore import remotemetadatastore, unionmetadatastore


# corresponds to uncompressed length of revlog's indexformatng (2 gigs, 4-byte
# signed integer)
_maxentrysize = 0x7FFFFFFF


class remotefilelognodemap(object):
    def __init__(self, filename, store):
        self._filename = filename
        self._store = store

    def __contains__(self, node):
        missing = self._store.getmissing([(self._filename, node)])
        return not bool(missing)

    def __get__(self, node):
        if node not in self:
            raise KeyError(node)
        return node


class remotefilelog(object):
    def __init__(self, opener, path, repo):
        self.opener = opener
        self.filename = path
        self.repo = repo
        self.nodemap = remotefilelognodemap(self.filename, repo.fileslog.contentstore)

        self.version = 1

    def read(self, node):
        """returns the file contents at this node"""
        t = self.revision(node)
        if not t.startswith("\1\n"):
            return t
        s = t.index("\1\n", 2)
        return t[s + 2 :]

    def add(self, text, meta, transaction, linknode, p1=None, p2=None):
        hashtext = text

        # hash with the metadata, like in vanilla filelogs
        hashtext = shallowutil.createrevlogtext(
            text, meta.get("copy"), meta.get("copyrev")
        )
        node = revlog.hash(hashtext, p1, p2)
        return self.addrevision(hashtext, transaction, linknode, p1, p2, node=node)

    def addrevision(
        self,
        text,
        transaction,
        linknode,
        p1,
        p2,
        cachedelta=None,
        node=None,
        flags=revlog.REVIDX_DEFAULT_FLAGS,
    ):
        # text passed to "addrevision" includes hg filelog metadata header
        if node is None:
            node = revlog.hash(text, p1, p2)

        meta, metaoffset = filelog.parsemeta(text)
        rawtext, validatehash = self._processflags(text, flags, "write")

        if len(rawtext) > _maxentrysize:
            raise revlog.RevlogError(
                _("%s: size of %s exceeds maximum size of %s")
                % (
                    self.filename,
                    util.bytecount(len(rawtext)),
                    util.bytecount(_maxentrysize),
                )
            )

        return self.addrawrevision(
            rawtext,
            transaction,
            linknode,
            p1,
            p2,
            node,
            flags,
            cachedelta,
            _metatuple=(meta, metaoffset),
        )

    def addrawrevision(
        self,
        rawtext,
        transaction,
        linknode,
        p1,
        p2,
        node,
        flags,
        cachedelta=None,
        _metatuple=None,
    ):
        if _metatuple:
            # _metatuple: used by "addrevision" internally by remotefilelog
            # meta was parsed confidently
            #
            # NOTE: meta is the "filelog" meta, which contains "copyrev"
            # information. It's *incompatible* with datapack meta, which is
            # about file size and revlog flags.
            meta, metaoffset = _metatuple
        else:
            # Not from self.addrevision, but something else (repo._filecommit)
            # calls addrawrevision directly. remotefilelog needs to get the
            # copy metadata via parsing it.
            meta, unused = shallowutil.parsemeta(rawtext, flags)

        dpack, hpack = self.repo.fileslog.getmutablelocalpacks()

        dpackmeta = {constants.METAKEYFLAG: flags}
        dpack.add(self.filename, node, revlog.nullid, rawtext, metadata=dpackmeta)

        copyfrom = ""
        realp1node = p1
        if meta and "copy" in meta:
            copyfrom = meta["copy"]
            realp1node = bin(meta["copyrev"])
        hpack.add(self.filename, node, realp1node, p2, linknode, copyfrom)

        return node

    def renamed(self, node):
        p1, p2, linknode, copyfrom = self.getnodeinfo(node)
        if copyfrom:
            return (copyfrom, p1)

        return False

    def size(self, node):
        """return the size of a given revision"""
        return len(self.read(node))

    rawsize = size

    def candelta(self, basenode, node):
        # Do not use delta if either node is LFS. Avoids issues if clients have
        # the delta base stored in different forms: one LFS, one non-LFS.
        if self.flags(basenode) or self.flags(node):
            return False
        # Do not use delta if "node" is a copy. This avoids cycles (in a graph
        # where edges are node -> deltabase, and node -> copyfrom). The cycle
        # could make remotefilelog cgunpacker enter an infinite loop.
        if self.renamed(node):
            return False
        return True

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
        if self.filename == ".hgtags":
            # The length of .hgtags is used to fast path tag checking.
            # remotefilelog doesn't support .hgtags since the entire .hgtags
            # history is needed.  Use the excludepattern setting to make
            # .hgtags a normal filelog.
            return 0

        raise RuntimeError("len not supported")

    def empty(self):
        return False

    def flags(self, node):
        if isinstance(node, int):
            raise error.ProgrammingError(
                "remotefilelog does not accept integer rev for flags"
            )
        if node == nullid:
            return revlog.REVIDX_DEFAULT_FLAGS
        store = self.repo.fileslog.contentstore
        return store.getmeta(self.filename, node).get(constants.METAKEYFLAG, 0)

    def parents(self, node):
        if node == nullid:
            return nullid, nullid

        p1, p2, linknode, copyfrom = self.repo.fileslog.metadatastore.getnodeinfo(
            self.filename, node
        )
        if copyfrom:
            p1 = nullid

        return p1, p2

    def linknode(self, node):
        p1, p2, linknode, copyfrom = self.repo.fileslog.metadatastore.getnodeinfo(
            self.filename, node
        )
        return linknode

    def revdiff(self, node1, node2):
        if node1 != nullid and (self.flags(node1) or self.flags(node2)):
            raise error.ProgrammingError("cannot revdiff revisions with non-zero flags")
        return mdiff.textdiff(
            self.revision(node1, raw=True), self.revision(node2, raw=True)
        )

    def lookup(self, node):
        if len(node) == 40:
            node = bin(node)
        if len(node) != 20:
            raise error.LookupError(node, self.filename, _("invalid lookup input"))

        return node

    def rev(self, node):
        # This is a hack to make TortoiseHG work.
        return node

    def node(self, rev):
        # This is a hack.
        if isinstance(rev, int):
            raise error.ProgrammingError(
                "remotefilelog does not convert integer rev to node"
            )
        return rev

    def revision(self, node, raw=False):
        """returns the revlog contents at this node.
        this includes the meta data traditionally included in file revlogs.
        this is generally only used for bundling and communicating with vanilla
        hg clients.
        """
        if node == nullid:
            return ""
        if len(node) != 20:
            raise error.LookupError(node, self.filename, _("invalid revision input"))

        store = self.repo.fileslog.contentstore
        rawtext = store.get(self.filename, node)
        if raw:
            return rawtext
        if rawtext == constants.REDACTED_CONTENT:
            return constants.REDACTED_MESSAGE
        flags = store.getmeta(self.filename, node).get(constants.METAKEYFLAG, 0)
        if flags == 0:
            return rawtext
        text, verifyhash = self._processflags(rawtext, flags, "read")
        return text

    def _deltachain(self, node):
        """Obtain the delta chain for a revision.

        Return (chain, False), chain is a list of nodes. This is to be
        compatible with revlog API.
        """
        store = self.repo.fileslog.contentstore
        chain = store.getdeltachain(self.filename, node)
        return ([x[1] for x in chain], False)

    def _processflags(self, text, flags, operation, raw=False):
        # mostly copied from hg/mercurial/revlog.py
        validatehash = True
        orderedflags = revlog.REVIDX_FLAGS_ORDER
        if operation == "write":
            orderedflags = reversed(orderedflags)
        for flag in orderedflags:
            if flag & flags:
                vhash = True
                if flag not in revlog._flagprocessors:
                    message = _("missing processor for flag '%#x'") % (flag)
                    raise revlog.RevlogError(message)
                readfunc, writefunc, rawfunc = revlog._flagprocessors[flag]
                if raw:
                    vhash = rawfunc(self, text)
                elif operation == "read":
                    text, vhash = readfunc(self, text)
                elif operation == "write":
                    text, vhash = writefunc(self, text)
                validatehash = validatehash and vhash
        return text, validatehash

    def ancestormap(self, node):
        return self.repo.fileslog.metadatastore.getancestors(self.filename, node)

    def getnodeinfo(self, node):
        return self.repo.fileslog.metadatastore.getnodeinfo(self.filename, node)

    def ancestor(self, a, b):
        if a == nullid or b == nullid:
            return nullid

        revmap, parentfunc = self._buildrevgraph(a, b)
        nodemap = dict(((v, k) for (k, v) in revmap.iteritems()))

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
        nodemap = dict(((v, k) for (k, v) in revmap.iteritems()))

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
        queue = collections.deque(
            ((None, n) for n in parentsmap.iterkeys() if n not in allparents)
        )
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


class remotefileslog(filelog.fileslog):
    """Top level object representing all the file storage.

    Eventually all file access should go through this, but for now it's just
    used to handle remotefilelog writes.
    """

    def __init__(self, repo):
        super(remotefileslog, self).__init__(repo)
        self._mutablelocalpacks = mutablestores.pendingmutablepack(
            repo,
            lambda: shallowutil.getlocalpackpath(
                self.repo.svfs.vfs.base, constants.FILEPACK_CATEGORY
            ),
        )
        self._mutablesharedpacks = mutablestores.pendingmutablepack(
            repo,
            lambda: shallowutil.getcachepackpath(
                self.repo, constants.FILEPACK_CATEGORY
            ),
        )
        self.makeunionstores()

    def getmutablelocalpacks(self):
        return self._mutablelocalpacks.getmutablepack()

    def getmutablesharedpacks(self):
        return self._mutablesharedpacks.getmutablepack()

    def commitsharedpacks(self):
        """Persist the dirty data written to the shared packs."""
        dpackpath, hpackpath = self._mutablesharedpacks.commit()

        self.repo.fileservice.updatecache(dpackpath, hpackpath)

        self.contentstore.markforrefresh()
        self.metadatastore.markforrefresh()

    def commitpending(self):
        """Used in alternative filelog implementations to commit pending
        additions."""
        self._mutablelocalpacks.commit()
        self.commitsharedpacks()

    def abortpending(self):
        """Used in alternative filelog implementations to throw out pending
        additions."""
        self._mutablelocalpacks.abort()
        self.commitsharedpacks()

    def makeunionstores(self):
        """Union stores iterate the other stores and return the first result."""
        repo = self.repo
        self.shareddatastores = []
        self.sharedhistorystores = []
        self.localdatastores = []
        self.localhistorystores = []

        cachecontent = []
        cachemetadata = []
        localcontent = []
        localmetadata = []

        spackcontent, spackmetadata, lpackcontent, lpackmetadata = self.makepackstores()
        cachecontent += [spackcontent]
        cachemetadata += [spackmetadata]
        localcontent += [lpackcontent]
        localmetadata += [lpackmetadata]

        mutablelocalstore = mutablestores.mutabledatahistorystore(
            lambda: self._mutablelocalpacks
        )

        mutablesharedstore = mutablestores.mutabledatahistorystore(
            lambda: self._mutablesharedpacks
        )

        sharedcontentstores = [spackcontent, mutablesharedstore]
        sharedmetadatastores = [spackmetadata, mutablesharedstore]
        if self.ui.configbool("remotefilelog", "indexedlogdatastore"):
            path = shallowutil.getindexedlogdatastorepath(repo)
            mask = os.umask(0o002)
            try:
                store = revisionstore.indexedlogdatastore(path)
                sharedcontentstores.append(store)
                self.shareddatastores.append(store)
            finally:
                os.umask(mask)

        if self.ui.configbool("remotefilelog", "indexedloghistorystore"):
            path = shallowutil.getindexedloghistorystorepath(repo)
            mask = os.umask(0o002)
            try:
                store = revisionstore.indexedloghistorystore(path)
                sharedmetadatastores.append(store)
                self.sharedhistorystores.append(store)
            finally:
                os.umask(mask)

        sunioncontentstore = unioncontentstore(*sharedcontentstores)
        sunionmetadatastore = unionmetadatastore(
            *sharedmetadatastores, allowincomplete=True
        )
        remotecontent, remotemetadata = self.makeremotestores(
            sunioncontentstore, sunionmetadatastore
        )

        contentstores = (
            sharedcontentstores
            + cachecontent
            + localcontent
            + [mutablelocalstore, remotecontent]
        )
        metadatastores = (
            sharedmetadatastores
            + cachemetadata
            + localmetadata
            + [mutablelocalstore, remotemetadata]
        )

        # Instantiate union stores
        self.contentstore = unioncontentstore(*contentstores)
        self.metadatastore = unionmetadatastore(*metadatastores)

        self.localcontentstore = unioncontentstore(*self.localdatastores)
        self.localmetadatastore = unionmetadatastore(*self.localhistorystores)

        repo.fileservice.setstore(self.contentstore, self.metadatastore)
        shallowutil.reportpackmetrics(
            repo.ui,
            "filestore",
            spackcontent,
            spackmetadata,
            lpackcontent,
            lpackmetadata,
        )

    def makepackstores(self):
        """Packs are more efficient (to read from) cache stores."""
        repo = self.repo

        def makepackstore(datastores, historystores, packpath, deletecorrupt=False):
            packcontentstore = makedatapackstore(
                repo.ui, packpath, deletecorruptpacks=deletecorrupt
            )
            packmetadatastore = makehistorypackstore(
                repo.ui, packpath, deletecorruptpacks=deletecorrupt
            )
            datastores.append(packcontentstore)
            historystores.append(packmetadatastore)

            return packcontentstore, packmetadatastore

        # Instantiate pack stores
        spackpath = shallowutil.getcachepackpath(repo, constants.FILEPACK_CATEGORY)
        spackcontent, spackmetadata = makepackstore(
            self.shareddatastores,
            self.sharedhistorystores,
            spackpath,
            deletecorrupt=True,
        )

        lpackpath = shallowutil.getlocalpackpath(
            repo.svfs.vfs.base, constants.FILEPACK_CATEGORY
        )
        lpackcontent, lpackmetadata = makepackstore(
            self.localdatastores, self.localhistorystores, lpackpath
        )

        return (spackcontent, spackmetadata, lpackcontent, lpackmetadata)

    def makeremotestores(self, cachecontent, cachemetadata):
        """These stores fetch data from a remote server."""
        repo = self.repo

        remotecontent = remotecontentstore(repo.ui, repo.fileservice, cachecontent)
        remotemetadata = remotemetadatastore(repo.ui, repo.fileservice, cachemetadata)
        return remotecontent, remotemetadata
