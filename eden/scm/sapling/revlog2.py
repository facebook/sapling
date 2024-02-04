# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Revlog compatibility backed by modern storage backend."""


import hashlib
import pickle

from dataclasses import dataclass
from typing import Dict, Union

from . import error, mdiff, revlog, util
from .i18n import _

from .node import bbin, nullid, nullrev, wdirid
from .revlog import revlog as orig_revlog, textwithheader


RevlogError = error.RevlogError
LookupError = error.LookupError
ProgrammingError = error.ProgrammingError


@dataclass
class RevInfo:
    """Extra info associated with a revision"""

    node: bytes
    p1: int
    p2: int
    flags: int
    linkrev: int
    offset: int
    chunk_size: int
    full_size: int
    base: int


NULL_INFO = RevInfo(nullid, nullrev, nullrev, 0, nullrev, 0, 0, 0, nullrev)


class RevlogMeta:
    """Extra metadata required by Revlog for compatibility.

    Represented as:

      {path:
       {'node2rev': {node: rev},
        'revs': [(node, p1, p2, flags, linkrev, offset, chunk_size, full_size, base)]}}

    The data can be updated manually (ex. revlog2.addrevision, bundlrevlog uses
    index.insert to track temporary revisions), or derived from the storage for
    all commits in the changelog (ex. after writing to the storage without
    using revlog2).

    To integrate with transaction, call `flush` on transaction close/pending.
    """

    def __init__(self, path: str, repo, store):
        self._path = path
        self._dirty = False
        try:
            with open(self._path, "rb") as f:
                self._data = getattr(pickle, "load")(f)
        except FileNotFoundError:
            self._data = {}

        # Fill "missing" data with best guess.
        self.derive(repo, store)

    def derive(self, repo, store):
        """Derive self._data from commits.

        Note: The order of (p1, p2) might be "wrong" since the actual order is
        not recorded in the "store".
        """
        cl = repo.changelog
        if len(cl) > 100000:
            raise error.Abort(
                _(
                    "revlog2 is for legacy test compatibility, and should not be used in large repos"
                )
            )
        nodes = [
            n for n in cl.dag.all().iterrev() if not self.hasnode("00changelog", n)
        ]

        def path_to_revlog_path(ty, path):
            if ty == "commit":
                return "00changelog"
            elif ty == "blob":
                return f"data/{path}"
            elif ty == "tree":
                if path == "":
                    return "00manifest"
                else:
                    return f"meta/{path}/00manifest"

        def skip(ty, path, node):
            return self.hasnode(path_to_revlog_path(ty, path), node)

        linkrev = nullrev
        for ty, path, node, p1node, p2node, flags in visit_blobs(store, nodes, skip):
            if ty == "commit":
                linkrev = cl.rev(node)
            path = path_to_revlog_path(ty, path)
            p1rev = self.rev(path, p1node)
            p2rev = self.rev(path, p2node)
            self.add(path, node, p1rev, p2rev, flags, linkrev)

        if "00manifest" in self._data:
            self._data["00manifesttree"] = self._data["00manifest"]

    def flush(self):
        if self._dirty:
            with open(self._path, "wb") as f:
                getattr(pickle, "dump")(self._data, f)
            self._dirty = False

    def add(
        self,
        path: str,
        node: bytes,
        p1: int,
        p2: int,
        flags: int,
        linkrev: int,
        offset=0,
        chunk_size=0,
        full_size=0,
        base=nullrev,
    ) -> int:
        """return the rev number"""
        data = self._data.get(path)
        if not data:
            data = self._data[path] = {"node2rev": {}, "revs": []}
        existing_rev = data["node2rev"].get(node)
        if existing_rev is not None:
            return existing_rev
        new_rev = len(data["revs"])
        data["revs"].append(
            (node, p1, p2, flags, linkrev, offset, chunk_size, full_size, base)
        )
        data["node2rev"][node] = new_rev
        self._dirty = True
        return new_rev

    def rev(self, path: str, node: bytes) -> int:
        if node == nullid:
            return nullrev
        return self._data[path]["node2rev"][node]

    def node(self, path: str, rev: int) -> bytes:
        if rev == nullrev:
            return nullid
        return self.revinfo(path, rev).node

    def nodemap(self, path: str) -> Dict[bytes, int]:
        data = self._data.get(path)
        if not data:
            result = {}
        else:
            result = data["node2rev"]
        if nullid not in result:
            result[nullid] = nullrev
        return result

    def hasnode(self, path: str, node: bytes) -> bool:
        if node == nullid:
            return True
        data = self._data.get(path)
        return bool(data and node in data["node2rev"])

    def revlen(self, path: str) -> int:
        if path not in self._data:
            return 0
        return len(self._data[path]["revs"])

    def revinfo(self, path: str, rev: int) -> RevInfo:
        if rev == nullrev:
            return NULL_INFO
        info = self._data[path]["revs"][rev]
        return RevInfo(*info)


def visit_blobs(store, nodes, skip):
    """Similar to exchange.findblobs, but:

    - Takes only EagerRepoStore without other dependencies.
      (which might infinite loop)
    - Different order: yield commit first, then tree and files.
    - Provide "flags".
    - Skip already visited nodes or when `skip(ty, path, node)` returns True.

    yield (blobtype, path, node, p1, p2, flags)
    blobtype: "blob", "tree", or "commit"
    """

    def read(ty, path, node):
        # read from store: (p1, p2, flags, text) or None
        blob = store.get_sha1_blob(node)
        if blob is None:
            # files can be missing for shallow repos.
            return None
        p1 = blob[20:40]
        p2 = blob[:20]
        flags = 0
        text = blob[40:]
        if ty == "blob":
            if revlog.hash(text, p1, p2) != node:
                flags = revlog.REVIDX_EXTSTORED
        return p1, p2, flags, text

    def child_items(ty, path, text):
        # yield child items (ty, path, node)
        if ty == "commit":
            mfnode = bbin(text[:40])
            yield ("tree", "", mfnode)
        elif ty == "tree":
            for name, hexflag in [l.split(b"\0") for l in text.split(b"\n") if l]:
                name = name.decode()
                subpath = f"{path}/{name}" if path else name
                if hexflag.endswith(b"t"):
                    subtype = "tree"
                elif hexflag.endswith(b"m"):
                    continue
                else:
                    subtype = "blob"
                yield (subtype, subpath, bbin(hexflag[:40]))

    visited = set()
    for commit_node in nodes:
        to_visit = [("commit", "", commit_node)]
        while to_visit:
            ty, path, node = to_visit.pop()
            if (path, node) in visited:
                continue
            visited.add((path, node))
            if skip(ty, path, node):
                continue
            item = read(ty, path, node)
            if item is not None:
                p1node, p2node, flags, text = item
                yield ty, path, node, p1node, p2node, flags
                to_visit += list(child_items(ty, path, text))


def maybe_wrap_svfs(svfs):
    """add methods to svfs to access a cached version of EagerRepoStore and RevlogMeta

    Why svfs: The "revlog" API takes a "svfs" instead of "repo" for historical
    reasons. They do not take "repo".

    Why propertycache: The original "revlog" maintains individual states. The
    new "revlog" wants shared access to a central content and metadata storage.
    Therefore propertycache is helpful.
    """
    if hasattr(svfs, "_revlog_meta"):
        return

    class RevlogSvfs(svfs.__class__):
        @util.propertycache
        def _revlog_meta(self):
            repo = self._reporef()
            path = self.join("revlogmeta")
            return RevlogMeta(path, repo, self._revlog_store)

        @util.propertycache
        def _revlog_store(self):
            repo = self._reporef()
            sfmt = repo.storage_format()
            if sfmt == "revlog":
                return repo._rsrepo.eagerstore()
            elif sfmt == "remotefilelog":
                # If treemanifest is disabled, then 00manifest.i uses revlog.
                # The Rust repo will refuse giving us the eagerstore. So we
                # create one directly.
                from . import eagerepo

                return eagerepo.openstore(repo)
            else:
                raise ProgrammingError(
                    f"revlog2 should not be used by {sfmt} storage format"
                )

        def _revlog_invalidate(self):
            for name in ["_revlog_store", "_revlog_meta"]:
                self.__dict__.pop(name, None)

        def _revlog_flush(self):
            self._revlog_store.flush()
            self._revlog_meta.flush()

    svfs.__class__ = RevlogSvfs


class revlog2:
    """The revlog interface backed by EagerRepoStore and RevlogMeta.

    Writes are pending in memory till svfs._revlog_flush().
    """

    def __init__(
        self,
        svfs,
        indexfile,
        # Not used. Provided for compatibility.
        datafile=None,
        checkambig=False,
        mmaplargeindex=False,
        index2=False,
    ):
        maybe_wrap_svfs(svfs)

        self._path = indexfile[:-2]
        try:
            self._store = svfs._revlog_store
        except AttributeError:
            repo = svfs._reporef()
            raise AttributeError(repo.storage_format())
        self._meta = svfs._revlog_meta
        self._svfs = svfs

        # used for error messages
        self.indexfile = indexfile

        # used by ext/lfs
        self.opener = svfs

        # used by bundlerevlog
        self._cache = None

    def __len__(self) -> int:
        return self._meta.revlen(self._path)

    __iter__ = orig_revlog.__iter__

    revs = orig_revlog.revs

    def rev(self, node):
        try:
            return self._meta.rev(self._path, node)
        except KeyError:
            raise LookupError(node, self.indexfile, _("no node"))

    def flags(self, rev):
        return self._meta.revinfo(self._path, rev).flags

    def rawsize(self, rev):
        t = self.revision(rev, raw=True)
        return len(t)

    def size(self, rev: int) -> int:
        return len(self.revision(rev, raw=False))

    def linkrev(self, rev):
        return self._meta.revinfo(self._path, rev).linkrev

    def parentrevs(self, rev):
        info = self._meta.revinfo(self._path, rev)
        return info.p1, info.p2

    def node(self, rev):
        return self._meta.node(self._path, rev)

    def parents(self, node):
        prevs = self.parentrevs(self.rev(node))
        pnodes = tuple(map(self.node, prevs))
        return pnodes

    ancestors = orig_revlog.ancestors

    commonancestorsheads = orig_revlog.commonancestorsheads

    _match = orig_revlog._match

    lookup = orig_revlog.lookup

    cmp = orig_revlog.cmp

    candelta = orig_revlog.candelta

    def revdiff(self, rev1, rev2):
        if rev1 > -1 and (self.flags(rev1) or self.flags(rev2)):
            raise error.ProgrammingError("cannot revdiff revisions with non-zero flags")

        return mdiff.textdiff(
            self.revision(rev1, raw=True), self.revision(rev2, raw=True)
        )

    def revision(
        self,
        nodeorrev: "Union[int, bytes]",
        raw: bool = False,
    ) -> bytes:
        """return an uncompressed revision of a given node or revision
        number.

        raw - an optional argument specifying if the revision data is to be
        treated as raw data when applying flag transforms. 'raw' should be set
        to True when generating changegroups or in debug commands.
        """
        if isinstance(nodeorrev, int):
            rev = nodeorrev
            node = self.node(rev)
        else:
            node = nodeorrev
            rev = None

        if node == nullid:
            return b""

        rawtext = self._store.get_content(node)
        if rawtext is None:
            raise LookupError(node, self.indexfile, _("no node"))
        if raw:
            return rawtext

        if rev is None:
            rev = self.rev(node)
        flags = self.flags(rev)

        text, validatehash = self._processflags(rawtext, flags, "read", raw=raw)
        if validatehash:
            self.checkhash(text, node, rev=rev)

        return text

    hash = orig_revlog.hash

    _processflags = orig_revlog._processflags

    checkhash = orig_revlog.checkhash

    def hasnode(self, node):
        return self._meta.hasnode(self._path, node)

    def _contains(self, node, p1node=None, p2node=None):
        return self.hasnode(node)

    addrevision = orig_revlog.addrevision

    def addrawrevision(self, rawtext, tr, link, p1, p2, node, flags, cachedelta=None):
        return self._addrevision(node, rawtext, tr, link, p1, p2, flags)

    def _addrevision(self, node, rawtext, tr, link, p1, p2, flags):
        assert node != nullid and node != wdirid

        if isinstance(rawtext, memoryview):
            rawtext = bytes(rawtext)

        self._meta.add(self._path, node, self.rev(p1), self.rev(p2), flags, link)
        blob = textwithheader(rawtext, p1, p2)

        if hashlib.sha1(blob).digest() != node:
            assert flags != 0
            self._store.add_arbitrary_blob(node, blob)
        else:
            new_node = self._store.add_sha1_blob(blob)
            assert new_node == node

        self._update_transaction(tr)
        return node

    def addgroup(self, deltas, linkmapper, tr):
        nodes = []
        for data in deltas:
            node, p1, p2, linknode, deltabase, delta, flags = data
            link = linkmapper(linknode)
            flags = flags or 0

            nodes.append(node)
            if self.hasnode(node):
                continue

            basetext = self.revision(deltabase, raw=True)
            rawtext = mdiff.patch(basetext, delta)

            self._addrevision(
                node,
                rawtext,
                tr,
                link,
                p1,
                p2,
                flags,
            )

        return nodes

    def _update_transaction(self, tr):
        flush = self._svfs._revlog_flush
        tr.addpending("revlog2", lambda tr: flush())
        tr.addfinalize("revlog2", lambda tr: flush())

    def deltaparent(self, rev):
        return nullrev

    @property
    def nodemap(self):
        return self._meta.nodemap(self._path)

    # used by remotefilelogserver, bundlerepo
    @property
    def index(self):
        return Index(self)

    # used by bundlerevlog
    def _chunk(self, rev):
        return self.revdiff(rev, nullrev)

    # used by bundlerevlog
    def start(self, rev):
        info = self._meta.revinfo(self._path, rev)
        return info.offset

    # used by bundlerevlog
    def length(self, rev):
        info = self._meta.revinfo(self._path, rev)
        return info.chunk_size

    # used by debugindex
    def chainbase(self, rev):
        return rev

    iscensored = orig_revlog.iscensored

    storedeltachains = False

    # used by debugindex
    version = 2


class Index:
    def __init__(self, inner: revlog2):
        self._inner = inner

    def __getitem__(self, rev: int):
        inner = self._inner
        info = inner._meta.revinfo(inner._path, rev)
        # start_flags, size, full unc. size, base (unused), link, p1, p2, node
        return (
            (info.offset << 16) | info.flags,
            info.chunk_size,
            info.full_size,
            info.base,
            info.linkrev,
            info.p1,
            info.p2,
            info.node,
        )

    def insert(self, _rev: int, entry):
        # Used by the bundlerevlog to insert new revs.
        # bundlerevlog needs the "offset" to be tracked here.
        offset_flags, size, full_size, base, link, p1, p2, node = entry[:8]
        inner = self._inner
        return inner._meta.add(
            inner._path,
            node=node,
            p1=p1,
            p2=p2,
            flags=offset_flags & 0xFFFF,
            linkrev=link,
            offset=offset_flags >> 16,
            chunk_size=size,
            full_size=full_size,
            base=base,
        )


@util.call_once()
def patch_types():
    """replace revlog with revlog2 for revlog's subclasses"""
    # Ensure those types are seen.
    from .bundlerepo import bundlerevlog
    from .filelog import filelog
    from .manifest import manifestrevlog

    for klass in orig_revlog.__subclasses__():
        bases = tuple((t is orig_revlog and revlog2 or t for t in klass.__bases__))
        klass.__bases__ = bases
