# Portions Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# changegroup.py - Mercurial changegroup manipulation functions
#
#  Copyright 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import os
import struct
import tempfile
import weakref

from . import (
    dagutil,
    error,
    mdiff,
    mutation,
    perftrace,
    phases,
    progress,
    pycompat,
    util,
    visibility,
)
from .i18n import _
from .node import hex, nullrev, short
from .pycompat import range


CFG_CGDELTA_ALWAYS_NULL = "always-null"
CFG_CGDELTA_NO_EXTERNAL = "no-external"
CFG_CGDELTA_DEFAULT = "default"

_CHANGEGROUPV1_DELTA_HEADER = "20s20s20s20s"
_CHANGEGROUPV2_DELTA_HEADER = "20s20s20s20s20s"
_CHANGEGROUPV3_DELTA_HEADER = ">20s20s20s20s20sH"


def readexactly(stream, n):
    """read n bytes from stream.read and abort if less was available"""
    s = stream.read(n)
    if len(s) < n:
        raise error.Abort(
            _("stream ended unexpectedly" " (got %d bytes, expected %d)") % (len(s), n)
        )
    return s


def getchunk(stream):
    """return the next chunk from stream as a string"""
    d = readexactly(stream, 4)
    l = struct.unpack(">l", d)[0]
    if l <= 4:
        if l:
            raise error.Abort(_("invalid chunk length %d") % l)
        return ""
    return readexactly(stream, l - 4)


def chunkheader(length):
    """return a changegroup chunk header (string)"""
    return struct.pack(">l", length + 4)


def closechunk():
    """return a changegroup chunk header (string) for a zero-length chunk"""
    return struct.pack(">l", 0)


def writechunks(ui, chunks, filename, vfs=None):
    """Write chunks to a file and return its filename.

    The stream is assumed to be a bundle file.
    Existing files will not be overwritten.
    If no filename is specified, a temporary file is created.
    """
    fh = None
    cleanup = None
    try:
        if filename:
            if vfs:
                fh = vfs.open(filename, "wb")
            else:
                # Increase default buffer size because default is usually
                # small (4k is common on Linux).
                fh = open(filename, "wb", 131072)
        else:
            fd, filename = tempfile.mkstemp(prefix="hg-bundle-", suffix=".hg")
            fh = util.fdopen(fd, pycompat.sysstr("wb"))
        cleanup = filename
        for c in chunks:
            fh.write(c)
        cleanup = None
        return filename
    finally:
        if fh is not None:
            fh.close()
        if cleanup is not None:
            if filename and vfs:
                vfs.unlink(cleanup)
            else:
                os.unlink(cleanup)


def checkrevs(repo, revs):
    # to be replaced by extensions
    # free from extension logic
    pass


class cg1unpacker(object):
    """Unpacker for cg1 changegroup streams.

    A changegroup unpacker handles the framing of the revision data in
    the wire format. Most consumers will want to use the apply()
    method to add the changes from the changegroup to a repository.

    If you're forwarding a changegroup unmodified to another consumer,
    use getchunks(), which returns an iterator of changegroup
    chunks. This is mostly useful for cases where you need to know the
    data stream has ended by observing the end of the changegroup.

    deltachunk() is useful only if you're applying delta data. Most
    consumers should prefer apply() instead.

    A few other public methods exist. Those are used only for
    bundlerepo and some debug commands - their use is discouraged.
    """

    deltaheader = _CHANGEGROUPV1_DELTA_HEADER
    deltaheadersize = struct.calcsize(deltaheader)
    version = "01"
    _grouplistcount = 1  # One list of files after the manifests

    def __init__(self, fh, alg, extras=None):
        if alg is None:
            alg = "UN"
        if alg not in util.compengines.supportedbundletypes:
            raise error.Abort(_("unknown stream compression type: %s") % alg)
        if alg == "BZ":
            alg = "_truncatedBZ"

        compengine = util.compengines.forbundletype(alg)
        self._stream = compengine.decompressorreader(fh)
        self._type = alg
        self.extras = extras or {}
        self.progress = None

    # These methods (compressed, read, seek, tell) all appear to only
    # be used by bundlerepo, but it's a little hard to tell.
    def compressed(self):
        return self._type is not None and self._type != "UN"

    def read(self, l):
        return self._stream.read(l)

    def seek(self, pos):
        return self._stream.seek(pos)

    def tell(self):
        return self._stream.tell()

    def close(self):
        return self._stream.close()

    def _chunklength(self):
        d = readexactly(self._stream, 4)
        l = struct.unpack(">l", d)[0]
        if l <= 4:
            if l:
                raise error.Abort(_("invalid chunk length %d") % l)
            return 0
        if self.progress is not None:
            self.progress.value += 1
        return l - 4

    def changelogheader(self):
        """v10 does not have a changelog header chunk"""
        return {}

    def manifestheader(self):
        """v10 does not have a manifest header chunk"""
        return {}

    def filelogheader(self):
        """return the header of the filelogs chunk, v10 only has the filename"""
        l = self._chunklength()
        if not l:
            return {}
        fname = readexactly(self._stream, l)
        return {"filename": fname}

    def _deltaheader(self, headertuple, prevnode):
        node, p1, p2, cs = headertuple
        if prevnode is None:
            deltabase = p1
        else:
            deltabase = prevnode
        flags = 0
        return node, p1, p2, deltabase, cs, flags

    def deltachunk(self, prevnode):
        l = self._chunklength()
        if not l:
            return {}
        headerdata = readexactly(self._stream, self.deltaheadersize)
        header = struct.unpack(self.deltaheader, headerdata)
        delta = readexactly(self._stream, l - self.deltaheadersize)
        node, p1, p2, deltabase, cs, flags = self._deltaheader(header, prevnode)
        return (node, p1, p2, cs, deltabase, delta, flags)

    def getchunks(self):
        """returns all the chunks contains in the bundle

        Used when you need to forward the binary stream to a file or another
        network API. To do so, it parse the changegroup data, otherwise it will
        block in case of sshrepo because it don't know the end of the stream.
        """
        # For changegroup 1 and 2, we expect 3 parts: changelog, manifestlog,
        # and a list of filelogs. For changegroup 3, we expect 4 parts:
        # changelog, manifestlog, a list of tree manifestlogs, and a list of
        # filelogs.
        #
        # Changelog and manifestlog parts are terminated with empty chunks. The
        # tree and file parts are a list of entry sections. Each entry section
        # is a series of chunks terminating in an empty chunk. The list of these
        # entry sections is terminated in yet another empty chunk, so we know
        # we've reached the end of the tree/file list when we reach an empty
        # chunk that was proceeded by no non-empty chunks.

        parts = 0
        while parts < 2 + self._grouplistcount:
            noentries = True
            while True:
                chunk = getchunk(self)
                if not chunk:
                    # The first two empty chunks represent the end of the
                    # changelog and the manifestlog portions. The remaining
                    # empty chunks represent either A) the end of individual
                    # tree or file entries in the file list, or B) the end of
                    # the entire list. It's the end of the entire list if there
                    # were no entries (i.e. noentries is True).
                    if parts < 2:
                        parts += 1
                    elif noentries:
                        parts += 1
                    break
                noentries = False
                yield chunkheader(len(chunk))
                pos = 0
                while pos < len(chunk):
                    next = pos + 2 ** 20
                    yield chunk[pos:next]
                    pos = next
            yield closechunk()

    def _unpackmanifests(self, repo, revmap, trp, numchanges):
        # We know that we'll never have more manifests than we had
        # changesets.
        with progress.bar(repo.ui, _("manifests"), total=numchanges) as prog:
            self.progress = prog
            # no need to check for empty manifest group here:
            # if the result of the merge of 1 and 2 is the same in 3 and 4,
            # no new manifest will be created and the manifest group will
            # be empty during the pull
            self.manifestheader()
            deltas = self.deltaiter()
            mfnodes = repo.manifestlog._revlog.addgroup(deltas, revmap, trp)
        self.progress = None
        return mfnodes

    def apply(
        self, repo, tr, srctype, url, targetphase=phases.draft, expectedtotal=None
    ):
        """Add the changegroup returned by source.read() to this repo.
        srctype is a string like 'push', 'pull', or 'unbundle'.  url is
        the URL of the repo where this changegroup is coming from.

        Return an integer summarizing the change to this repo:
        - nothing changed or no source: 0
        - more heads than before: 1+added heads (2..n)
        - fewer heads than before: -1-removed heads (-2..-n)
        - number of heads stays the same: 1
        """
        repo = repo.unfiltered()

        def csmap(x):
            repo.ui.debug("add changeset %s\n" % short(x))
            return len(cl)

        def revmap(x):
            return cl.rev(x)

        changesets = files = revisions = 0

        try:
            # The transaction may already carry source information. In this
            # case we use the top level data. We overwrite the argument
            # because we need to use the top level value (if they exist)
            # in this function.
            srctype = tr.hookargs.setdefault("source", srctype)
            url = tr.hookargs.setdefault("url", url)
            repo.hook("prechangegroup", throw=True, **pycompat.strkwargs(tr.hookargs))

            # write changelog data to temp files so concurrent readers
            # will not see an inconsistent view
            cl = repo.changelog
            cl.delayupdate(tr)

            trp = weakref.proxy(tr)
            # pull off the changeset group
            repo.ui.status(_("adding changesets\n"))
            clstart = len(cl)
            with progress.bar(repo.ui, _("changesets"), total=expectedtotal) as prog:
                self.progress = prog
                efiles = set()

                def onchangelog(cl, node):
                    efiles.update(cl.readfiles(node))

                self.changelogheader()
                deltas = self.deltaiter()
                cgnodes = cl.addgroup(deltas, csmap, trp, addrevisioncb=onchangelog)
                efiles = len(efiles)

            perftrace.tracevalue("Commits", len(cgnodes))
            if cgnodes:
                perftrace.tracevalue(
                    "Range", "%s:%s" % (hex(cgnodes[0])[:12], hex(cgnodes[-1])[:12])
                )
            self.progress = None

            if not cgnodes:
                repo.ui.develwarn(
                    "applied empty changegroup", config="warn-empty-changegroup"
                )
            clend = len(cl)
            changesets = clend - clstart

            # pull off the manifest group
            repo.ui.status(_("adding manifests\n"))
            self._unpackmanifests(repo, revmap, trp, changesets)

            needfiles = {}
            if repo.ui.configbool("server", "validate"):
                cl = repo.changelog
                ml = repo.manifestlog
                # validate incoming csets have their manifests
                for cset in range(clstart, clend):
                    mfnode = cl.changelogrevision(cset).manifest
                    mfest = ml[mfnode].readnew()
                    # store file cgnodes we must see
                    for f, n in mfest.iteritems():
                        needfiles.setdefault(f, set()).add(n)

            # process the files
            repo.ui.status(_("adding file changes\n"))
            newrevs, newfiles = _addchangegroupfiles(
                repo, self, revmap, trp, efiles, needfiles
            )
            revisions += newrevs
            files += newfiles

            htext = ""

            repo.ui.status(
                _("added %d changesets" " with %d changes to %d files%s\n")
                % (changesets, revisions, files, htext)
            )
            repo.invalidatevolatilesets()

            if changesets > 0:
                if "node" not in tr.hookargs:
                    tr.hookargs["node"] = hex(cl.node(clstart))
                    tr.hookargs["node_last"] = hex(cl.node(clend - 1))
                    hookargs = dict(tr.hookargs)
                else:
                    hookargs = dict(tr.hookargs)
                    hookargs["node"] = hex(cl.node(clstart))
                    hookargs["node_last"] = hex(cl.node(clend - 1))
                repo.hook(
                    "pretxnchangegroup", throw=True, **pycompat.strkwargs(hookargs)
                )

            added = [cl.node(r) for r in range(clstart, clend)]
            phaseall = None
            if srctype in ("push", "serve"):
                # Old servers can not push the boundary themselves.
                # New servers won't push the boundary if changeset already
                # exists locally as secret
                #
                # We should not use added here but the list of all change in
                # the bundle
                if repo.publishing():
                    targetphase = phaseall = phases.public
                else:
                    # closer target phase computation

                    # Those changesets have been pushed from the
                    # outside, their phases are going to be pushed
                    # alongside. Therefor `targetphase` is
                    # ignored.
                    targetphase = phaseall = phases.draft
            if added:
                phases.registernew(repo, tr, targetphase, added)
                if targetphase > phases.public:
                    visibility.add(repo, added)
                    if mutation.recording(repo):
                        entries = [
                            mutation.createcommitentry(repo, node) for node in added
                        ]
                        entries = [
                            entry.tostoreentry()
                            for entry in entries
                            if entry is not None
                        ]
                        mutation.recordentries(repo, entries, skipexisting=False)

            if phaseall is not None:
                phases.advanceboundary(repo, tr, phaseall, cgnodes)

            if changesets > 0:

                def runhooks():
                    # These hooks run when the lock releases, not when the
                    # transaction closes. So it's possible for the changelog
                    # to have changed since we last saw it.
                    if clstart >= len(repo):
                        return

                    repo.hook("changegroup", **pycompat.strkwargs(hookargs))

                tr.addpostclose(
                    "changegroup-runhooks-%020i" % clstart,
                    lambda tr: repo._afterlock(runhooks),
                )

                checkrevs(repo, range(clstart, clend))
        finally:
            repo.ui.flush()
        # never return 0 here:
        ret = 1
        return ret

    def deltaiter(self):
        """
        returns an iterator of the deltas in this changegroup

        Useful for passing to the underlying storage system to be stored.
        """
        chain = None
        for chunkdata in iter(lambda: self.deltachunk(chain), {}):
            # Chunkdata: (node, p1, p2, cs, deltabase, delta, flags)
            yield chunkdata
            chain = chunkdata[0]


class cg2unpacker(cg1unpacker):
    """Unpacker for cg2 streams.

    cg2 streams add support for generaldelta, so the delta header
    format is slightly different. All other features about the data
    remain the same.
    """

    deltaheader = _CHANGEGROUPV2_DELTA_HEADER
    deltaheadersize = struct.calcsize(deltaheader)
    version = "02"

    def _deltaheader(self, headertuple, prevnode):
        node, p1, p2, deltabase, cs = headertuple
        flags = 0
        return node, p1, p2, deltabase, cs, flags


class cg3unpacker(cg2unpacker):
    """Unpacker for cg3 streams.

    cg3 streams add support for exchanging treemanifests and revlog
    flags. It adds the revlog flags to the delta header and an empty chunk
    separating manifests and files.
    """

    deltaheader = _CHANGEGROUPV3_DELTA_HEADER
    deltaheadersize = struct.calcsize(deltaheader)
    version = "03"
    _grouplistcount = 2  # One list of manifests and one list of files

    def _deltaheader(self, headertuple, prevnode):
        node, p1, p2, deltabase, cs, flags = headertuple
        return node, p1, p2, deltabase, cs, flags

    def _unpackmanifests(self, repo, revmap, trp, numchanges):
        mfnodes = super(cg3unpacker, self)._unpackmanifests(
            repo, revmap, trp, numchanges
        )
        for chunkdata in iter(self.filelogheader, {}):
            # If we get here, there are directory manifests in the changegroup
            d = chunkdata["filename"]
            repo.ui.debug("adding %s revisions\n" % d)
            dirlog = repo.manifestlog._revlog.dirlog(d)
            deltas = self.deltaiter()
            if not dirlog.addgroup(deltas, revmap, trp):
                raise error.Abort(_("received dir revlog group is empty"))

        return mfnodes


class headerlessfixup(object):
    def __init__(self, fh, h):
        self._h = h
        self._fh = fh

    def read(self, n):
        if self._h:
            d, self._h = self._h[:n], self._h[n:]
            if len(d) < n:
                d += readexactly(self._fh, n - len(d))
            return d
        return readexactly(self._fh, n)


class cg1packer(object):
    deltaheader = _CHANGEGROUPV1_DELTA_HEADER
    version = "01"

    def __init__(self, repo, bundlecaps=None, b2caps=None):
        """Given a source repo, construct a bundler.

        bundlecaps is optional and can be used to specify the set of
        capabilities which can be used to build the bundle. While bundlecaps is
        unused in core Mercurial, extensions rely on this feature to communicate
        capabilities to customize the changegroup packer.
        """
        # Set of capabilities we can use to build the bundle.
        if bundlecaps is None:
            bundlecaps = set()
        if b2caps is None:
            b2caps = {}
        self._bundlecaps = bundlecaps
        self._b2caps = b2caps
        # experimental config: bundle.reorder
        reorder = repo.ui.config("bundle", "reorder")
        if reorder == "auto":
            reorder = None
        else:
            reorder = util.parsebool(reorder)
        self._repo = repo
        self._reorder = reorder
        if self._repo.ui.verbose and not self._repo.ui.debugflag:
            self._verbosenote = self._repo.ui.note
        else:
            self._verbosenote = lambda s: None
        cgdeltaconfig = repo.ui.config("format", "cgdeltabase")
        if cgdeltaconfig not in [
            CFG_CGDELTA_ALWAYS_NULL,
            CFG_CGDELTA_NO_EXTERNAL,
            CFG_CGDELTA_DEFAULT,
        ]:
            repo.ui.warn(_("ignore unknown cgdeltabase config: %s\n") % cgdeltaconfig)
            cgdeltaconfig = CFG_CGDELTA_DEFAULT
        self._cgdeltaconfig = cgdeltaconfig

    def close(self):
        return closechunk()

    def fileheader(self, fname):
        return chunkheader(len(fname)) + fname

    # Extracted both for clarity and for overriding in extensions.
    def _sortgroup(self, revlog, nodelist, lookup):
        """Sort nodes for change group and turn them into revnums."""
        # for generaldelta revlogs, we linearize the revs; this will both be
        # much quicker and generate a much smaller bundle
        if (revlog._generaldelta and self._reorder is None) or self._reorder:
            dag = dagutil.revlogdag(revlog)
            return dag.linearize(set(revlog.rev(n) for n in nodelist))
        else:
            return sorted([revlog.rev(n) for n in nodelist])

    def group(self, nodelist, revlog, lookup, prog=None):
        """Calculate a delta group, yielding a sequence of changegroup chunks
        (strings).

        Given a list of changeset revs, return a set of deltas and
        metadata corresponding to nodes. The first delta is
        first parent(nodelist[0]) -> nodelist[0], the receiver is
        guaranteed to have this parent as it has all history before
        these changesets. In the case firstparent is nullrev the
        changegroup starts with a full revision.

        If prog is not None, its value attribute will be updated with progress.
        """
        # if we don't have any revisions touched by these changesets, bail
        if len(nodelist) == 0:
            yield self.close()
            return

        revs = self._sortgroup(revlog, nodelist, lookup)

        # add the parent of the first rev
        p = revlog.parentrevs(revs[0])[0]
        revs.insert(0, p)

        # build deltas
        if prog is not None:
            prog._total = len(revs) - 1
        for r in range(len(revs) - 1):
            if prog is not None:
                prog.value = r + 1
            prev, curr = revs[r], revs[r + 1]
            linknode = lookup(revlog.node(curr))
            if self._cgdeltaconfig == CFG_CGDELTA_ALWAYS_NULL:
                prev = nullrev
            elif self._cgdeltaconfig == CFG_CGDELTA_NO_EXTERNAL and r == 0:
                prev = nullrev
            for c in self.revchunk(revlog, curr, prev, linknode):
                yield c

        yield self.close()

    # filter any nodes that claim to be part of the known set
    def prune(self, revlog, missing, commonrevs):
        rr, rl = revlog.rev, revlog.linkrev
        return [n for n in missing if rl(rr(n)) not in commonrevs]

    def _packmanifests(self, dir, mfnodes, lookuplinknode):
        """Pack flat manifests into a changegroup stream."""
        assert not dir
        with progress.bar(self._repo.ui, _("bundling"), _("manifests")) as prog:
            for chunk in self.group(
                mfnodes, self._repo.manifestlog._revlog, lookuplinknode, prog
            ):
                yield chunk

    def _manifestsdone(self):
        return ""

    def generate(self, commonrevs, clnodes, fastpathlinkrev, source):
        """yield a sequence of changegroup chunks (strings)"""
        repo = self._repo
        cl = repo.changelog

        clrevorder = {}
        mfs = {}  # needed manifests
        fnodes = {}  # needed file nodes
        changedfiles = set()

        # Callback for the changelog, used to collect changed files and manifest
        # nodes.
        # Returns the linkrev node (identity in the changelog case).
        def lookupcl(x):
            c = cl.read(x)
            clrevorder[x] = len(clrevorder)
            n = c[0]
            # record the first changeset introducing this manifest version
            mfs.setdefault(n, x)
            # Record a complete list of potentially-changed files in
            # this manifest.
            changedfiles.update(c[3])
            return x

        self._verbosenote(_("uncompressed size of bundle content:\n"))
        size = 0
        with progress.bar(repo.ui, _("bundling"), _("changesets")) as prog:
            for chunk in self.group(clnodes, cl, lookupcl, prog):
                size += len(chunk)
                yield chunk
        self._verbosenote(_("%8.i (changelog)\n") % size)

        # We need to make sure that the linkrev in the changegroup refers to
        # the first changeset that introduced the manifest or file revision.
        # The fastpath is usually safer than the slowpath, because the filelogs
        # are walked in revlog order.
        #
        # When taking the slowpath with reorder=None and the manifest revlog
        # uses generaldelta, the manifest may be walked in the "wrong" order.
        # Without 'clrevorder', we would get an incorrect linkrev (see fix in
        # cc0ff93d0c0c).
        #
        # When taking the fastpath, we are only vulnerable to reordering
        # of the changelog itself. The changelog never uses generaldelta, so
        # it is only reordered when reorder=True. To handle this case, we
        # simply take the slowpath, which already has the 'clrevorder' logic.
        # This was also fixed in cc0ff93d0c0c.
        fastpathlinkrev = fastpathlinkrev and not self._reorder
        # Treemanifests don't work correctly with fastpathlinkrev
        # either, because we don't discover which directory nodes to
        # send along with files. This could probably be fixed.
        fastpathlinkrev = fastpathlinkrev and ("treemanifest" not in repo.requirements)

        for chunk in self.generatemanifests(
            commonrevs, clrevorder, fastpathlinkrev, mfs, fnodes, source
        ):
            yield chunk
        mfs.clear()
        clrevs = set(cl.rev(x) for x in clnodes)

        if not fastpathlinkrev:

            def linknodes(unused, fname):
                return fnodes.get(fname, {})

        else:
            cln = cl.node

            def linknodes(filerevlog, fname):
                llr = filerevlog.linkrev
                fln = filerevlog.node
                revs = ((r, llr(r)) for r in filerevlog)
                return dict((fln(r), cln(lr)) for r, lr in revs if lr in clrevs)

        for chunk in self.generatefiles(changedfiles, linknodes, commonrevs, source):
            yield chunk

        yield self.close()

        if clnodes:
            repo.hook("outgoing", node=hex(clnodes[0]), source=source)

    def generatemanifests(
        self, commonrevs, clrevorder, fastpathlinkrev, mfs, fnodes, source
    ):
        """Returns an iterator of changegroup chunks containing manifests.

        `source` is unused here, but is used by extensions like remotefilelog to
        change what is sent based in pulls vs pushes, etc.
        """
        repo = self._repo
        mfl = repo.manifestlog
        dirlog = mfl._revlog.dirlog
        tmfnodes = {"": mfs}

        # Callback for the manifest, used to collect linkrevs for filelog
        # revisions.
        # Returns the linkrev node (collected in lookupcl).
        def makelookupmflinknode(dir, nodes):
            if fastpathlinkrev:
                assert not dir
                return mfs.__getitem__

            def lookupmflinknode(x):
                """Callback for looking up the linknode for manifests.

                Returns the linkrev node for the specified manifest.

                SIDE EFFECT:

                1) fclnodes gets populated with the list of relevant
                   file nodes if we're not using fastpathlinkrev
                2) When treemanifests are in use, collects treemanifest nodes
                   to send

                Note that this means manifests must be completely sent to
                the client before you can trust the list of files and
                treemanifests to send.
                """
                clnode = nodes[x]
                mfctx = mfl.get(dir, x)
                mdata = mfctx.readnew(shallow=True)
                for p, n, fl in mdata.iterentries():
                    if fl == "t":  # subdirectory manifest
                        subdir = dir + p + "/"
                        tmfclnodes = tmfnodes.setdefault(subdir, {})
                        tmfclnode = tmfclnodes.setdefault(n, clnode)
                        if clrevorder[clnode] < clrevorder[tmfclnode]:
                            tmfclnodes[n] = clnode
                    else:
                        f = dir + p
                        fclnodes = fnodes.setdefault(f, {})
                        fclnode = fclnodes.setdefault(n, clnode)
                        if clrevorder[clnode] < clrevorder[fclnode]:
                            fclnodes[n] = clnode
                return clnode

            return lookupmflinknode

        size = 0
        while tmfnodes:
            dir, nodes = tmfnodes.popitem()
            prunednodes = self.prune(dirlog(dir), nodes, commonrevs)
            if not dir or prunednodes:
                for x in self._packmanifests(
                    dir, prunednodes, makelookupmflinknode(dir, nodes)
                ):
                    size += len(x)
                    yield x
        self._verbosenote(_("%8.i (manifests)\n") % size)
        yield self._manifestsdone()

    # The 'source' parameter is useful for extensions
    def generatefiles(self, changedfiles, linknodes, commonrevs, source):
        repo = self._repo
        total = len(changedfiles)
        with progress.bar(repo.ui, _("bundling"), _("files"), total) as prog:
            for i, fname in enumerate(sorted(changedfiles)):
                filerevlog = repo.file(fname)
                if not filerevlog:
                    msg = _("empty or missing revlog for %s") % fname
                    raise error.Abort(msg)

                linkrevnodes = linknodes(filerevlog, fname)
                # Lookup for filenodes, we collected the linkrev nodes above in
                # the fastpath case and with lookupmf in the slowpath case.
                def lookupfilelog(x):
                    return linkrevnodes[x]

                filenodes = self.prune(filerevlog, linkrevnodes, commonrevs)
                if filenodes:
                    prog.value = (i + 1, fname)
                    h = self.fileheader(fname)
                    size = len(h)
                    yield h
                    for chunk in self.group(filenodes, filerevlog, lookupfilelog):
                        size += len(chunk)
                        yield chunk
                    self._verbosenote(_("%8.i  %s\n") % (size, fname))

    def deltaparent(self, revlog, rev, p1, p2, prev):
        if not revlog.candelta(prev, rev) or self._cgdeltaconfig != CFG_CGDELTA_DEFAULT:
            raise error.ProgrammingError("cannot change deltabase for cg1")
        return prev

    def revchunk(self, revlog, rev, prev, linknode):
        node = revlog.node(rev)
        p1, p2 = revlog.parentrevs(rev)
        base = self.deltaparent(revlog, rev, p1, p2, prev)

        prefix = ""
        if revlog.iscensored(base) or revlog.iscensored(rev):
            try:
                delta = revlog.revision(node, raw=True)
            except error.CensoredNodeError as e:
                delta = e.tombstone
            if base == nullrev:
                prefix = mdiff.trivialdiffheader(len(delta))
            else:
                baselen = revlog.rawsize(base)
                prefix = mdiff.replacediffheader(baselen, len(delta))
        elif base == nullrev:
            delta = revlog.revision(node, raw=True)
            prefix = mdiff.trivialdiffheader(len(delta))
        else:
            delta = revlog.revdiff(base, rev)
        p1n, p2n = revlog.parents(node)
        basenode = revlog.node(base)
        flags = revlog.flags(rev)
        meta = self.builddeltaheader(node, p1n, p2n, basenode, linknode, flags)
        meta += prefix
        l = len(meta) + len(delta)
        yield chunkheader(l)
        yield meta
        yield delta

    def builddeltaheader(self, node, p1n, p2n, basenode, linknode, flags):
        # do nothing with basenode, it is implicitly the previous one in HG10
        # do nothing with flags, it is implicitly 0 for cg1 and cg2
        return struct.pack(self.deltaheader, node, p1n, p2n, linknode)


class cg2packer(cg1packer):
    version = "02"
    deltaheader = _CHANGEGROUPV2_DELTA_HEADER

    def __init__(self, repo, bundlecaps=None, b2caps=None):
        super(cg2packer, self).__init__(repo, bundlecaps, b2caps=b2caps)
        if self._reorder is None:
            # Since generaldelta is directly supported by cg2, reordering
            # generally doesn't help, so we disable it by default (treating
            # bundle.reorder=auto just like bundle.reorder=False).
            self._reorder = False

    def deltaparent(self, revlog, rev, p1, p2, prev):
        if self._cgdeltaconfig == CFG_CGDELTA_DEFAULT:
            dp = revlog.deltaparent(rev)
        else:
            dp = nullrev
        if dp == nullrev and revlog.storedeltachains:
            # Avoid sending full revisions when delta parent is null. Pick prev
            # in that case. It's tempting to pick p1 in this case, as p1 will
            # be smaller in the common case. However, computing a delta against
            # p1 may require resolving the raw text of p1, which could be
            # expensive. The revlog caches should have prev cached, meaning
            # less CPU for changegroup generation. There is likely room to add
            # a flag and/or config option to control this behavior.
            base = prev
        elif dp == nullrev:
            # revlog is configured to use full snapshot for a reason,
            # stick to full snapshot.
            base = nullrev
        elif dp not in (p1, p2, prev):
            # Pick prev when we can't be sure remote has the base revision.
            base = prev
        else:
            base = dp
        if base != nullrev and not revlog.candelta(base, rev):
            base = nullrev
        return base

    def builddeltaheader(self, node, p1n, p2n, basenode, linknode, flags):
        # Do nothing with flags, it is implicitly 0 in cg1 and cg2
        return struct.pack(self.deltaheader, node, p1n, p2n, basenode, linknode)


class cg3packer(cg2packer):
    version = "03"
    deltaheader = _CHANGEGROUPV3_DELTA_HEADER

    def _packmanifests(self, dir, mfnodes, lookuplinknode):
        if dir:
            yield self.fileheader(dir)

        dirlog = self._repo.manifestlog._revlog.dirlog(dir)
        with progress.bar(self._repo.ui, _("bundling"), _("manifests")) as prog:
            for chunk in self.group(mfnodes, dirlog, lookuplinknode, prog):
                yield chunk

    def _manifestsdone(self):
        return self.close()

    def builddeltaheader(self, node, p1n, p2n, basenode, linknode, flags):
        return struct.pack(self.deltaheader, node, p1n, p2n, basenode, linknode, flags)


_packermap = {
    "01": (cg1packer, cg1unpacker),
    # cg2 adds support for exchanging generaldelta
    "02": (cg2packer, cg2unpacker),
    # cg3 adds support for exchanging revlog flags and treemanifests
    "03": (cg3packer, cg3unpacker),
}


def allsupportedversions(repo):
    versions = set(_packermap.keys())
    if not (
        repo.ui.configbool("experimental", "changegroup3")
        or repo.ui.configbool("experimental", "treemanifest")
        or "treemanifest" in repo.requirements
    ):
        versions.discard("03")
    return versions


# Changegroup versions that can be applied to the repo
def supportedincomingversions(repo):
    return allsupportedversions(repo)


# Changegroup versions that can be created from the repo
def supportedoutgoingversions(repo):
    versions = allsupportedversions(repo)
    versions.discard("01")
    # developer config: format.allowbundle1
    if repo.ui.configbool("format", "allowbundle1") or "bundle1" in repo.ui.configlist(
        "devel", "legacy.exchange"
    ):
        versions.add("01")
    if "treemanifest" in repo.requirements:
        # Versions 01 and 02 support only flat manifests and it's just too
        # expensive to convert between the flat manifest and tree manifest on
        # the fly. Since tree manifests are hashed differently, all of history
        # would have to be converted. Instead, we simply don't even pretend to
        # support versions 01 and 02.
        versions.discard("01")
        versions.discard("02")
    return versions


def localversion(repo):
    # Finds the best version to use for bundles that are meant to be used
    # locally, such as those from strip and shelve, and temporary bundles.
    return max(supportedoutgoingversions(repo))


def safeversion(repo):
    # Finds the smallest version that it's safe to assume clients of the repo
    # will support. For example, all hg versions that support generaldelta also
    # support changegroup 02.
    versions = supportedoutgoingversions(repo)
    if "generaldelta" in repo.requirements:
        versions.discard("01")
    assert versions
    return min(versions)


def getbundler(version, repo, bundlecaps=None, b2caps=None):
    assert version in supportedoutgoingversions(repo)
    return _packermap[version][0](repo, bundlecaps, b2caps=b2caps)


def getunbundler(version, fh, alg, extras=None):
    return _packermap[version][1](fh, alg, extras=extras)


def _changegroupinfo(repo, nodes, source):
    if repo.ui.verbose or source == "bundle":
        repo.ui.status(_("%d changesets found\n") % len(nodes))
    if repo.ui.debugflag:
        repo.ui.debug("list of changesets:\n")
        for node in nodes:
            repo.ui.debug("%s\n" % hex(node))


def makechangegroup(
    repo, outgoing, version, source, fastpath=False, bundlecaps=None, b2caps=None
):
    cgstream = makestream(
        repo,
        outgoing,
        version,
        source,
        fastpath=fastpath,
        bundlecaps=bundlecaps,
        b2caps=b2caps,
    )
    return getunbundler(
        version, util.chunkbuffer(cgstream), None, {"clcount": len(outgoing.missing)}
    )


def makestream(
    repo, outgoing, version, source, fastpath=False, bundlecaps=None, b2caps=None
):
    if version == "01":
        repo.ui.develwarn("using deprecated bundlev1 format\n")

    bundler = getbundler(version, repo, bundlecaps=bundlecaps, b2caps=b2caps)

    repo = repo.unfiltered()
    commonrevs = outgoing.common
    csets = outgoing.missing

    if repo.ui.configbool("experimental", "narrow-heads"):
        # repo.heads() can no longer provide accurate hints about whether
        # 'fastpathlinkrev' should be used or not.
        # If 'fastpathlinkrev' gets accidentally set to True, tests like
        # like test-visibility-cloudsync.t will fail due to missing files
        # in bundles.
        fastpathlinkrev = False
    else:
        heads = outgoing.missingheads
        # We go through the fast path if we get told to, or if all (unfiltered
        # heads have been requested (since we then know there all linkrevs will
        # be pulled by the client).
        heads.sort()
        fastpathlinkrev = fastpath or (
            repo.filtername is None and heads == sorted(repo.heads())
        )

    repo.hook("preoutgoing", throw=True, source=source)
    _changegroupinfo(repo, csets, source)
    return bundler.generate(commonrevs, csets, fastpathlinkrev, source)


def _addchangegroupfiles(repo, source, revmap, trp, expectedfiles, needfiles):
    revisions = 0
    files = 0
    with progress.bar(repo.ui, _("files"), _("files"), expectedfiles) as prog:
        for chunkdata in iter(source.filelogheader, {}):
            files += 1
            f = chunkdata["filename"]
            repo.ui.debug("adding %s revisions\n" % f)
            prog.value = files
            fl = repo.file(f)
            o = len(fl)
            try:
                deltas = source.deltaiter()
                if not fl.addgroup(deltas, revmap, trp):
                    raise error.Abort(_("received file revlog group is empty"))
            except error.CensoredBaseError as e:
                raise error.Abort(_("received delta base is censored: %s") % e)
            revisions += len(fl) - o
            if f in needfiles:
                needs = needfiles[f]
                for new in range(o, len(fl)):
                    n = fl.node(new)
                    if n in needs:
                        needs.remove(n)
                    else:
                        raise error.Abort(_("received spurious file revlog entry"))
                if not needs:
                    del needfiles[f]

    for f, needs in needfiles.iteritems():
        fl = repo.file(f)
        for n in needs:
            try:
                fl.rev(n)
            except error.LookupError:
                raise error.Abort(
                    _("missing file data for %s:%s - run hg verify") % (f, hex(n))
                )

    return revisions, files
