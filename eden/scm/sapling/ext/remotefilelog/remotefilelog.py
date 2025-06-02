# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# remotefilelog.py - filelog implementation where filelog history is stored
#                    remotely


import collections
import os

from bindings import revisionstore

from sapling import ancestor, error, filelog, mdiff, revlog, util
from sapling.i18n import _
from sapling.node import bin, hex, nullid

from . import constants, shallowutil


class remotefilelognodemap:
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


class remotefilelog:
    def __init__(self, opener, path, repo):
        self.opener = opener
        self.filename = path
        self.repo = repo
        self.nodemap = remotefilelognodemap(self.filename, repo.fileslog.filestore)

        self.version = 1

    def read(self, node):
        """returns the file contents at this node"""
        t = self.revision(node)
        if not t.startswith(b"\1\n"):
            return t
        s = t.index(b"\1\n", 2)
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

        if self._reusefilenode(node, p1, p2):
            self.repo.ui.debug("reusing remotefilelog node %s\n" % hex(node))
            return node

        meta, metaoffset = filelog.parsemeta(text)
        rawtext, validatehash = self._processflags(text, flags, "write")

        softlimit = self.repo.ui.configbytes("commit", "file-size-limit", "1GB")
        hardlimit = self.repo.ui.configbytes("devel", "hard-file-size-limit", "10GB")
        limit = min(softlimit, hardlimit)
        if len(rawtext) >= limit:
            hint = None
            if support := self.repo.ui.config("ui", "supportcontact"):
                hint = _("contact %s for help") % support
            if len(rawtext) < hardlimit:
                hint = _(" or ").join(
                    list(
                        filter(
                            None,
                            [
                                hint,
                                _(
                                    "use '--config commit.file-size-limit=N' to override"
                                ),
                            ],
                        )
                    )
                )
            raise error.Abort(
                _("%s: size of %s exceeds maximum size of %s!")
                % (
                    self.filename,
                    util.bytecount(len(rawtext)),
                    util.bytecount(limit),
                ),
                hint=hint,
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

    def _reusefilenode(self, node, p1, p2) -> bool:
        if not self.repo.ui.configbool("experimental", "reuse-filenodes", True):
            return False

        # Similar to localrepo._filecommit, skip "nodemap" check assuming that if the
        # node's parents (via getlocalnodeinfo) are available that means filenide is also
        # available.
        if (
            not self.repo.ui.configbool(
                "experimental", "infer-filenode-available", True
            )
            and node not in self.nodemap
        ):
            return False

        localinfo = self.repo.fileslog.metadatastore.getlocalnodeinfo(
            self.filename, node
        )
        if localinfo is None:
            return False

        lp1, lp2, _, copyfrom = localinfo
        if copyfrom:
            lp1 = nullid

        return (p1, p2) == (lp1, lp2)

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

        dpack.add(
            self.filename,
            node,
            revlog.nullid,
            rawtext,
            metadata={constants.METAKEYFLAG: flags},
        )

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
        try:
            meta = self.repo.fileslog.filestore.metadata(self.filename, node)
            return meta["size"]
        except KeyError:
            pass

        return len(self.read(node))

    rawsize = size

    def candelta(self, basenode, node):
        """No delta support for remotefilelog."""
        return False

    def cmp(self, node, text):
        """compare text with a given file revision

        returns True if text is different than what is stored.
        """

        if node == nullid:
            return True

        # If it appears to be a redacted file, do a full comparison. Normally
        # we'd do a flags comparison, but the flags coming from Mononoke in the
        # tests don't seem to include the redacted flag.
        if text == constants.REDACTED_MESSAGE:
            return self.read(node) != text

        # remotefilectx.cmp uses the size as a shortcircuit. Unfortunately the
        # size comparison is expensive for lfs files, since reading the size
        # from the store currently also involves reading the content.
        #
        # The content comparison is expensive as well, since we have to load
        # the content from the store and from disk. Let's just check the
        # node instead.
        try:
            p1, p2, linknode, copyfrom = self.repo.fileslog.metadatastore.getnodeinfo(
                self.filename, node
            )
        except KeyError:
            # for subtree copy, we are reusing the old filelog node but with a new filename,
            # so the key (filename, node) is not in the metadatastore, let's just do a full
            # comparison.
            return self.read(node) != text

        if copyfrom or text.startswith(b"\1\n"):
            meta = {}
            if copyfrom:
                meta["copy"] = copyfrom
                meta["copyrev"] = hex(p1)
                p1 = nullid
            text = filelog.packmeta(meta, text)

        newnode = revlog.hash(text, p1, p2)
        return node != newnode

    def __nonzero__(self):
        return True

    __bool__ = __nonzero__

    def __len__(self):
        raise RuntimeError("len not supported")

    def empty(self):
        return False

    def flags(self, node):
        return 0

    def parents(self, node):
        if node == nullid:
            return nullid, nullid

        p1, p2, linknode, copyfrom = self.repo.fileslog.metadatastore.getnodeinfo(
            self.filename, node
        )
        if copyfrom:
            p1 = nullid

        return p1, p2

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
            return b""
        if len(node) != 20:
            raise error.LookupError(node, self.filename, _("invalid revision input"))

        rawtext = self.repo.fileslog.get(self.filename, node)
        if raw:
            return rawtext
        if rawtext == constants.REDACTED_CONTENT:
            return constants.REDACTED_MESSAGE
        return rawtext

    def _deltachain(self, node):
        """Obtain the delta chain for a revision.

        Return (chain, False), chain is a list of nodes. This is to be
        compatible with revlog API.
        """
        store = self.repo.fileslog.filestore
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
                    message = _("missing processor for flag '%#x'") % flag
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

    def _getancestors(self, node):
        """Returns as many ancestors as we're aware of.

        return value: {
           node: (p1, p2, linknode, copyfrom),
           ...
        }

        This is a very expansive operation as it requires the entire history
        for the node, potentially requiring O(N) server roundtrips.
        """
        known = set()
        ancestors = {}

        def traverse(curname, curnode):
            # TODO: this algorithm has the potential to traverse parts of
            # history twice. Ex: with A->B->C->F and A->B->D->F, both D and C
            # may be queued as missing, then B and A are traversed for both.
            queue = [(curname, curnode)]
            missing = []
            seen = set()
            while queue:
                name, node = queue.pop()
                if (name, node) in seen:
                    continue
                seen.add((name, node))
                value = ancestors.get(node)
                if not value:
                    missing.append((name, node))
                    continue
                p1, p2, linknode, copyfrom = value
                if p1 != nullid and p1 not in known:
                    queue.append((copyfrom or name, p1))
                if p2 != nullid and p2 not in known:
                    queue.append((name, p2))
            return missing

        missing = [(self.filename, node)]
        while missing:
            curname, curnode = missing.pop()
            try:
                # Prefetch full history since we are walking it.
                self.repo.fileslog.metadatastore.prefetch([(curname, curnode)])

                ancestors.update(
                    {
                        curnode: self.repo.fileslog.metadatastore.getnodeinfo(
                            curname, curnode
                        )
                    }
                )
                newmissing = traverse(curname, curnode)
                missing.extend(newmissing)
            except KeyError:
                raise

        # TODO: ancestors should probably be (name, node) -> (value)
        return ancestors

    def ancestormap(self, node):
        return self._getancestors(node)

    def getnodeinfo(self, node):
        return self.repo.fileslog.metadatastore.getnodeinfo(self.filename, node)

    def ancestor(self, a, b):
        if a == nullid or b == nullid:
            return nullid

        revmap, parentfunc = self._buildrevgraph(a, b)
        nodemap = dict(((v, k) for (k, v) in revmap.items()))

        ancs = ancestor.ancestors(parentfunc, revmap[a], revmap[b])
        if ancs:
            # choose a consistent winner when there's a tie
            return min(list(map(nodemap.__getitem__, ancs)))
        return nullid

    def commonancestorsheads(self, a, b):
        """calculate all the heads of the common ancestors of nodes a and b"""

        if a == nullid or b == nullid:
            return nullid

        revmap, parentfunc = self._buildrevgraph(a, b)
        nodemap = dict(((v, k) for (k, v) in revmap.items()))

        ancs = ancestor.commonancestorsheads(parentfunc, revmap[a], revmap[b])
        return list(map(nodemap.__getitem__, ancs))

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
            for node, pdata in mapping.items():
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
            ((None, n) for n in parentsmap.keys() if n not in allparents)
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
        self.makeruststore(repo)
        self._content_cache = util.lrucachedict(100)

    def makeruststore(self, repo):
        mask = os.umask(0o002)
        try:
            self.filestore = repo._rsrepo.filescmstore()
            edenapi = repo.nullableedenapi
            self.metadatastore = revisionstore.metadatastore(
                repo.svfs.base,
                repo.ui._rcfg,
                edenapi.filestore() if edenapi else None,
            )
        finally:
            os.umask(mask)

    def getmutablelocalpacks(self):
        return self.filestore, self.metadatastore

    def commitsharedpacks(self):
        """Persist the dirty data written to the shared packs."""
        self.filestore = None
        self.metadatastore = None
        self.makeruststore(self.repo)

    def commitpending(self):
        """Used in alternative filelog implementations to commit pending
        additions."""

        if self.filestore:
            self.filestore.flush()
            self.logfetches()

        if self.metadatastore:
            self.metadatastore.flush()
        self.commitsharedpacks()

    def abortpending(self):
        """Used in alternative filelog implementations to throw out pending
        additions."""
        self.logfetches()
        self.filestore = None
        self.metadatastore = None

    def logfetches(self):
        # TODO(meyer): Rename this function

        if not self.filestore:
            return

        ui = self.repo.ui
        if type(self.filestore) is revisionstore.filescmstore:
            metrics = self.filestore.getmetrics()
            for metric, value in metrics:
                ui.metrics.gauge(metric, value)

    TEN_MB = 10 * 1024**2

    def get(self, filename, node) -> bytes:
        data = self._content_cache.get(node, None)
        if data is not None:
            return data

        data = self.filestore.get(filename, node)
        if len(data) < self.TEN_MB:
            self._content_cache[node] = data
        return data
