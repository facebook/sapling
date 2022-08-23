# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# remotefilelog.py - filelog implementation where filelog history is stored
#                    remotely
from __future__ import absolute_import

import collections
import os

from bindings import revisionstore
from edenscm.mercurial import ancestor, error, filelog, mdiff, pycompat, revlog, util
from edenscm.mercurial.i18n import _
from edenscm.mercurial.node import bin, hex, nullid
from edenscm.mercurial.pycompat import isint

from .. import clienttelemetry
from . import constants, fileserverclient, shallowutil
from .repack import fulllocaldatarepack


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
        try:
            meta = self.repo.fileslog.contentstore.metadata(self.filename, node)
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
        p1, p2, linknode, copyfrom = self.repo.fileslog.metadatastore.getnodeinfo(
            self.filename, node
        )

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
        if isint(node):
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
        if isint(rev):
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
        nodemap = dict(((v, k) for (k, v) in pycompat.iteritems(revmap)))

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
        nodemap = dict(((v, k) for (k, v) in pycompat.iteritems(revmap)))

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
            for node, pdata in pycompat.iteritems(mapping):
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
            ((None, n) for n in pycompat.iterkeys(parentsmap) if n not in allparents)
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
        self._memcachestore = None
        self._edenapistore = None

        def needmaintenance(fname: str) -> bool:
            if repo.svfs.exists(fname):
                tstamp = int(repo.svfs.readutf8(fname))
                return tstamp < repo.ui.configint(
                    "remotefilelog", "maintenance.timestamp.%s" % fname
                )
            else:
                return True

        def markmaintenancedone(fname):
            with repo.lock():
                repo.svfs.writeutf8(
                    fname,
                    str(
                        repo.ui.configint(
                            "remotefilelog", "maintenance.timestamp.%s" % fname
                        )
                    ),
                )

        maintenance = repo.ui.configlist("remotefilelog", "maintenance")
        if maintenance:
            for kind in maintenance:
                if needmaintenance(kind):
                    if kind == "localrepack":
                        with repo.ui.configoverride(
                            {("remotefilelog", "useextstored"): True}
                        ):
                            self.makeruststore(repo)
                            repo.ui.warn(
                                _(
                                    "Running a one-time local repack, this may take some time\n"
                                )
                            )
                            fulllocaldatarepack(
                                repo, (self.contentstore, self.metadatastore)
                            )
                            repo.ui.warn(_("Done with one-time local repack\n"))
                        markmaintenancedone(kind)
                    else:
                        repo.ui.warn(
                            _("Unknown config value: %s in remotefilelog.maintenance\n")
                            % kind
                        )

        self.makeruststore(repo)

    def memcachestore(self, repo):
        if self._memcachestore is None:
            if repo.ui.config("remotefilelog", "cachekey"):
                self._memcachestore = revisionstore.memcachestore(repo.ui._rcfg)

        return self._memcachestore

    def edenapistore(self, repo):
        if self._edenapistore is None:
            useedenapi = repo.ui.configbool("remotefilelog", "http")
            if repo.ui.config("ui", "ssh") == "false":
                # Cannot use ssh. Force EdenAPI.
                useedenapi = True
            if repo.nullableedenapi is not None and useedenapi:
                self._edenapistore = repo.edenapi.filestore()

        return self._edenapistore

    def makesharedonlyruststore(self, repo):
        """Build non-local stores.

        There are handful of cases where we need to force prefetch data
        that is present in the local store, for this specific case, let's
        build shared-only stores.

        Do not use it except in the fileserverclient.prefetch method!
        """

        sharedonlyremotestore = revisionstore.pyremotestore(
            fileserverclient.getpackclient(repo)
        )
        memcachestore = self.memcachestore(repo)
        edenapistore = self.edenapistore(repo)

        correlator = clienttelemetry.correlator(repo.ui)

        mask = os.umask(0o002)
        try:
            if repo.ui.configbool("scmstore", "enableshim"):
                sharedonlycontentstore = revisionstore.filescmstore(
                    None,
                    repo.ui._rcfg,
                    sharedonlyremotestore,
                    memcachestore,
                    edenapistore,
                    correlator=correlator,
                )
            else:
                sharedonlycontentstore = revisionstore.contentstore(
                    None,
                    repo.ui._rcfg,
                    sharedonlyremotestore,
                    memcachestore,
                    edenapistore,
                    correlator=correlator,
                )
            sharedonlymetadatastore = revisionstore.metadatastore(
                None,
                repo.ui._rcfg,
                sharedonlyremotestore,
                memcachestore,
                edenapistore,
            )
        finally:
            os.umask(mask)

        return sharedonlycontentstore, sharedonlymetadatastore

    def makeruststore(self, repo):
        remotestore = revisionstore.pyremotestore(fileserverclient.getpackclient(repo))

        memcachestore = self.memcachestore(repo)
        edenapistore = self.edenapistore(repo)

        correlator = clienttelemetry.correlator(repo.ui)

        mask = os.umask(0o002)
        try:
            self.filescmstore = revisionstore.filescmstore(
                repo.svfs.vfs.base,
                repo.ui._rcfg,
                remotestore,
                memcachestore,
                edenapistore,
                correlator=correlator,
            )
            if repo.ui.configbool("scmstore", "enableshim"):
                self.contentstore = self.filescmstore
            else:
                self.contentstore = self.filescmstore.get_contentstore()
            self.metadatastore = revisionstore.metadatastore(
                repo.svfs.vfs.base,
                repo.ui._rcfg,
                remotestore,
                memcachestore,
                edenapistore,
            )
        finally:
            os.umask(mask)

    def getmutablelocalpacks(self):
        return self.contentstore, self.metadatastore

    def commitsharedpacks(self):
        """Persist the dirty data written to the shared packs."""
        self.filescmstore = None
        self.contentstore = None
        self.metadatastore = None
        self.makeruststore(self.repo)

    def commitpending(self):
        """Used in alternative filelog implementations to commit pending
        additions."""
        if self.contentstore:
            self.contentstore.flush()
            self.logfetches()

        if self.filescmstore:
            if (
                not self.contentstore
                or type(self.contentstore) is not revisionstore.filescmstore
            ):
                self.filescmstore.flush()
                self.logfetches()

        if self.metadatastore:
            self.metadatastore.flush()
        self.commitsharedpacks()

    def abortpending(self):
        """Used in alternative filelog implementations to throw out pending
        additions."""
        self.logfetches()
        self.filescmstore = None
        self.contentstore = None
        self.metadatastore = None
        self._memcachestore = None

    def logfetches(self):
        # TODO(meyer): Rename this function
        ui = self.repo.ui
        if self.contentstore:
            fetched = self.contentstore.getloggedfetches()
            if fetched:
                for path in fetched:
                    ui.log(
                        "undesired_file_fetches",
                        "",
                        filename=path,
                        reponame=self.repo.name,
                    )
                ui.metrics.gauge("undesiredfilefetches", len(fetched))
        scmstore = None
        if self.contentstore and type(self.contentstore) is revisionstore.filescmstore:
            scmstore = self.contentstore
        elif self.filescmstore:
            scmstore = self.filescmstore
        if scmstore:
            metrics = self.filescmstore.getmetrics()
            for (metric, value) in metrics:
                ui.metrics.gauge(metric, value)
