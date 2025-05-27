# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# shallowbundle.py - bundle10 implementation for use with shallow repositories


from typing import Any, Iterable, Mapping, MutableMapping, Sequence

from sapling import (
    bundlerepo,
    changegroup,
    error,
    mdiff,
    phases,
    progress,
    revlog,
    util,
)
from sapling.i18n import _
from sapling.node import bin, hex, nullid

from . import remotefilelog, shallowutil

NoFiles = NoTrees = 0
# Local means: files and trees that are not available on the main server
LocalFiles = LocalTrees = 1
AllFiles = AllTrees = 2

requirement = "remotefilelog"


def shallowgroup(cls, self, nodelist, rlog, lookup, prog=None, reorder=None):
    if not isinstance(rlog, remotefilelog.remotefilelog):
        for c in super(cls, self).group(nodelist, rlog, lookup, prog=prog):
            yield c
        return

    if len(nodelist) == 0:
        yield self.close()
        return

    nodelist = shallowutil.sortnodes(nodelist, rlog.parents)

    # add the parent of the first rev
    p = rlog.parents(nodelist[0])[0]
    nodelist.insert(0, p)

    # build deltas
    for i in range(len(nodelist) - 1):
        prev, curr = nodelist[i], nodelist[i + 1]
        if self._cgdeltaconfig == changegroup.CFG_CGDELTA_ALWAYS_NULL:
            prev = nullid
        elif self._cgdeltaconfig == changegroup.CFG_CGDELTA_NO_EXTERNAL and i == 0:
            prev = nullid
        linknode = lookup(curr)
        for c in self.nodechunk(rlog, curr, prev, linknode):
            yield c

    yield self.close()


@shallowutil.interposeclass(changegroup, "cg1packer")
class shallowcg1packer(changegroup.cg1packer):
    def group(self, nodelist, revlog, lookup, prog=None, reorder=None):
        return shallowgroup(shallowcg1packer, self, nodelist, revlog, lookup, prog=prog)

    def _cansendflat(self, mfnodes):
        return False

    def generatemanifests(
        self,
        commonrevs: "Sequence[int]",
        clrevorder: "Mapping[bytes, int]",
        mfs: "Any",
        fnodes: "MutableMapping[str, Any]",
        source: "Any",
    ) -> "Iterable[bytes]":
        """
        - `commonrevs` is the set of known commits on both sides
        - `clrevorder` is a mapping from cl node to rev number, used for
                       determining which commit is newer.
        - `mfs` is the potential manifest nodes to send,
                with maps to their linknodes
                { manifest root node -> link node }
        - `fnodes` is a mapping of { filepath -> { node -> clnode } }
                we are responsible for populating fnodes.
        - `args` and `kwargs` are extra arguments that will be passed to the
                core generatemanifests method, whose length depends on the
                version of core Hg.
        """
        # If we're not using the fastpath, then all the trees will be necessary
        # so we can inspect which files changed and need to be sent. So let's
        # bulk fetch the trees up front.
        repo = self._repo

        if self._cansendflat(mfs.keys()):
            # In this code path, generating the manifests populates fnodes for
            # us.
            chunks = super(shallowcg1packer, self).generatemanifests(
                commonrevs, clrevorder, mfs, fnodes, source
            )
            for chunk in chunks:
                yield chunk
        else:
            # If we're sending files, we need to process the manifests
            filestosend = self.shouldaddfilegroups(source)
            if filestosend is not NoFiles:
                cl = repo.changelog
                clparents = cl.parents
                clrevision = cl.changelogrevision
                mflog = repo.manifestlog
                with progress.bar(repo.ui, _("manifests"), total=len(mfs)) as prog:
                    for mfnode, clnode in mfs.items():
                        prog.value += 1
                        if filestosend == LocalFiles and repo[clnode].ispublic():
                            continue
                        mfctx = mflog[mfnode]
                        clp1node = clparents(clnode)[0]
                        p1node = clrevision(clp1node).manifest
                        p1ctx = mflog[p1node]

                        diff = p1ctx.read().diff(mfctx.read()).items()
                        for filename, ((anode, aflag), (bnode, bflag)) in diff:
                            if bnode is not None:
                                fclnodes = fnodes.setdefault(filename, {})
                                fclnode = fclnodes.setdefault(bnode, clnode)
                                if clrevorder[clnode] < clrevorder[fclnode]:
                                    fclnodes[bnode] = clnode

            yield self.close()

    def generatefiles(self, changedfiles, linknodes, commonrevs, source):
        if self._repo.ui.configbool("remotefilelog", "server"):
            caps = self._bundlecaps or []
            if requirement in caps:
                changedfiles = []

        if requirement in self._repo.requirements:
            repo = self._repo
            if isinstance(repo, bundlerepo.bundlerepository):
                # If the bundle contains filelogs, we can't pull from it, since
                # bundlerepo is heavily tied to revlogs. Instead require that
                # the user use unbundle instead.
                # Force load the filelog data.
                bundlerepo.bundlerepository.file(repo, "foo")
                if repo._cgfilespos:
                    raise error.Abort(
                        "cannot pull from full bundles",
                        hint=_("use `@prog@ unbundle` instead"),
                    )
                return []
            filestosend = self.shouldaddfilegroups(source)
            if filestosend == NoFiles:
                changedfiles = []
            else:
                files = []

                phasecache = repo._phasecache
                cl = repo.changelog

                # Prefetch the revisions being bundled
                for i, fname in enumerate(sorted(changedfiles)):
                    filerevlog = repo.file(fname)
                    linkrevnodes = linknodes(filerevlog, fname)
                    # Normally we'd prune the linkrevnodes first,
                    # but that would perform the server fetches one by one.
                    for fnode, cnode in list(linkrevnodes.items()):
                        # Adjust linknodes so remote file revisions aren't sent
                        if filestosend == LocalFiles:
                            if phasecache.phase(repo, cl.rev(cnode)) == phases.public:
                                del linkrevnodes[fnode]
                            else:
                                files.append((fname, fnode))
                        else:
                            files.append((fname, fnode))

                repo.fileservice.prefetch(files)

                # Prefetch the revisions that are going to be diffed against
                prevfiles = []
                for fname, fnode in files:
                    filerevlog = repo.file(fname)
                    p1, p2, linknode, copyfrom = filerevlog.getnodeinfo(fnode)
                    if p1 != nullid:
                        prevfiles.append((copyfrom or fname, p1))

                repo.fileservice.prefetch(prevfiles)

        return super(shallowcg1packer, self).generatefiles(
            changedfiles, linknodes, commonrevs, source
        )

    def shouldaddfilegroups(self, source):
        repo = self._repo
        isclient = requirement in repo.requirements
        isserver = repo.ui.configbool("remotefilelog", "server")

        if not isclient and not isserver:
            return AllFiles

        if source == "push" or source == "bundle":
            return AllFiles

        caps = self._bundlecaps or []
        b2caps = self._b2caps or {}
        if source == "serve" or source == "pull" or source == "rebase:reply":
            if "remotefilelog" in caps or "True" in b2caps.get("remotefilelog", []):
                return LocalFiles
            else:
                # Serving to a full repo requires us to serve everything
                if isclient:
                    repo.ui.warn(_("pulling from a shallow repo\n"))
                return AllFiles

        if isclient:
            return NoFiles
        else:
            return AllFiles

    def prune(self, rlog, missing, commonrevs):
        if not isinstance(rlog, remotefilelog.remotefilelog):
            return super(shallowcg1packer, self).prune(rlog, missing, commonrevs)

        repo = self._repo
        results = []
        for fnode in missing:
            fctx = repo.filectx(rlog.filename, fileid=fnode)
            linkrev = fctx.linkrev()
            if linkrev == -1 or linkrev not in commonrevs:
                results.append(fnode)
        return results

    def pointer(self, meta, flog, node):
        """For an LFS blob, the data is uploaded via the LFS protocol, only
        write a pointer to it in the bundle.
        """
        pointer = (
            "version https://git-lfs.github.com/spec/v1\n"
            "oid sha256:%s\n"
            "size %d\n" % (hex(meta["sha256"]), meta["size"])
        )

        renamed = flog.renamed(node)
        if renamed:
            path, renamednode = renamed
            pointer += "x-hg-copy %s\nx-hg-copyrev %s\n" % (path, hex(renamednode))

        pointer += "x-is-binary %d\n" % meta["isbinary"]

        return pointer.encode()

    def nodechunk(self, flog, node, _prevnode, linknode):
        prefix = b""

        def getmeta():
            try:
                fileslog = flog.repo.fileslog
                meta = fileslog.filestore.metadata(flog.filename, node)
                return meta
            except KeyError:
                pass
            return None

        meta = getmeta()
        if meta is not None:
            delta = self.pointer(meta, flog, node)
            flags = revlog.REVIDX_EXTSTORED
        else:
            delta = flog.revision(node, raw=True)
            flags = flog.flags(node)

        prefix = mdiff.trivialdiffheader(len(delta))

        p1, p2 = flog.parents(node)
        # We always send the full content, no deltas are used.
        meta = self.builddeltaheader(node, p1, p2, nullid, linknode, flags)
        meta += prefix
        l = len(meta) + len(delta)
        yield changegroup.chunkheader(l)
        yield meta
        yield delta


if hasattr(changegroup, "cg2packer"):
    # Mercurial >= 3.3
    @shallowutil.interposeclass(changegroup, "cg2packer")
    class shallowcg2packer(changegroup.cg2packer, shallowcg1packer):
        def group(self, nodelist, revlog, lookup, prog=None, reorder=None):
            # for revlogs, shallowgroup will be called twice in the same stack
            # -- once here, once up the inheritance hierarchy in
            # shallowcg1packer. That's fine though because for revlogs,
            # shallowgroup doesn't do anything on top of the usual group
            # function. If that assumption changes this will have to be
            # revisited.
            return shallowgroup(
                shallowcg2packer, self, nodelist, revlog, lookup, prog=prog
            )


if hasattr(changegroup, "cg3packer"):

    @shallowutil.interposeclass(changegroup, "cg3packer")
    class shallowcg3packer(changegroup.cg3packer, shallowcg1packer):
        def generatemanifests(
            self, commonrevs, clrevorder, mfs, fnodes, *args, **kwargs
        ):
            chunks = super(shallowcg3packer, self).generatemanifests(
                commonrevs, clrevorder, mfs, fnodes, *args, **kwargs
            )
            for chunk in chunks:
                yield chunk

            # If we're not sending flat manifests, then the subclass
            # generatemanifests call did not add the appropriate closing chunk
            # for a changegroup3.
            if not self._cansendflat(mfs.keys()):
                yield self._manifestsdone()


def addchangegroupfiles(orig, repo, source, revmap, trp, *args):
    if not requirement in repo.requirements:
        return orig(repo, source, revmap, trp, *args)

    newfiles = 0
    visited = set()
    revisiondatas = {}
    queue = []

    # Normal Mercurial processes each file one at a time, adding all
    # the new revisions for that file at once. In remotefilelog a file
    # revision may depend on a different file's revision (in the case
    # of a rename/copy), so we must lay all revisions down across all
    # files in topological order.

    # read all the file chunks but don't add them
    with progress.bar(repo.ui, _("files")) as prog:
        while True:
            chunkdata = source.filelogheader()
            if not chunkdata:
                break
            f = chunkdata["filename"]
            repo.ui.debug("adding %s revisions\n" % f)
            prog.value += 1

            chain = None
            while True:
                # returns: (node, p1, p2, cs, deltabase, delta, flags) or None
                revisiondata = source.deltachunk(chain)
                if not revisiondata:
                    break

                chain = revisiondata[0]

                revisiondatas[(f, chain)] = revisiondata
                queue.append((f, chain))

                if f not in visited:
                    newfiles += 1
                    visited.add(f)

            if chain is None:
                raise error.Abort(_("received file revlog group is empty"))

    processed = set()

    def available(f, node, depf, depnode):
        if depnode != nullid and (depf, depnode) not in processed:
            if not (depf, depnode) in revisiondatas:
                # It's not in the changegroup, assume it's already
                # in the repo
                return True
            # re-add self to queue
            queue.insert(0, (f, node))
            # add dependency in front
            queue.insert(0, (depf, depnode))
            return False
        return True

    skipcount = 0

    # Prefetch the non-bundled revisions that we will need
    prefetchfiles = []
    for f, node in queue:
        revisiondata = revisiondatas[(f, node)]
        # revisiondata: (node, p1, p2, cs, deltabase, delta, flags)
        dependents = [revisiondata[1], revisiondata[2], revisiondata[4]]

        for dependent in dependents:
            if dependent == nullid or (f, dependent) in revisiondatas:
                continue
            prefetchfiles.append((f, dependent))

    repo.fileservice.prefetch(prefetchfiles)

    # Get rawtext by applying delta chains.
    @util.lrucachefunc
    def reconstruct(f, node):
        revisiondata = revisiondatas.get((f, node), None)
        if revisiondata is None:
            # Read from repo.
            return repo.file(f).revision(node, raw=False)
        else:
            # Apply delta-chain.
            # revisiondata: (node, p1, p2, cs, deltabase, delta, flags)
            deltabase, delta, flags = revisiondata[4:]
            if deltabase == nullid:
                base = ""
            else:
                if flags:
                    # LFS (flags != 0) should always use nullid as deltabase.
                    raise error.Abort("unexpected deltabase")
                base = reconstruct(f, deltabase)
            rawtext = mdiff.patch(base, delta)
            if isinstance(rawtext, memoryview):  # noqa
                rawtext = bytes(rawtext)
            return rawtext

    # Apply the revisions in topological order such that a revision
    # is only written once it's deltabase and parents have been written.
    maxskipcount = len(queue) + 1
    while queue:
        f, node = queue.pop(0)
        if (f, node) in processed:
            continue

        skipcount += 1
        if skipcount > maxskipcount:
            raise error.Abort(_("circular node dependency on ancestormap"))

        revisiondata = revisiondatas[(f, node)]
        # revisiondata: (node, p1, p2, cs, deltabase, delta, flags)
        node, p1, p2, linknode, deltabase, delta, flags = revisiondata

        # Deltas are always against flags=0 rawtext (see revdiff and its
        # callers), if deltabase is not nullid.
        if flags and deltabase != nullid:
            raise error.Abort("unexpected deltabase")

        rawtext = reconstruct(f, node)
        meta, text = shallowutil.parsemeta(rawtext, flags)
        if "copy" in meta:
            copyfrom = meta["copy"]
            copynode = bin(meta["copyrev"])
            if not available(f, node, copyfrom, copynode):
                continue

        if any(not available(f, node, f, p) for p in [p1, p2] if p != nullid):
            continue

        # Use addrawrevision so if it's already LFS, take it as-is, do not
        # re-calculate the LFS object.
        fl = repo.file(f)
        fl.addrawrevision(rawtext, trp, linknode, p1, p2, node=node, flags=flags)
        processed.add((f, node))
        skipcount = 0

    return len(revisiondatas), newfiles


def cansendtrees(repo, nodes, source=None, bundlecaps=None, b2caps=None):
    """Sending trees has the following rules:

    Clients:
    - Send draft trees

    Server:
    - Only send local trees (i.e. infinitepush trees only)

    The function also does a prefetch on clients, so all the necessary trees are
    bulk downloaded.
    """

    if b2caps is None:
        b2caps = {}
    if bundlecaps is None:
        bundlecaps = set()

    def clienthascap(cap):
        return cap in bundlecaps or "True" in b2caps.get(cap, [])

    if clienthascap("treemanifestserver"):
        # If we're talking to the main server, always send everything.
        result = AllTrees
        prefetch = AllTrees
    else:
        # If we are a client, don't send public commits since we probably
        # don't have the trees and since the destination client will be able
        # to fetch them on demand anyway. Servers should send them if
        # they're doing a push, but that should almost never happen.
        result = LocalTrees
        prefetch = LocalTrees

    ctxs = [repo[node] for node in nodes]

    try:
        repo.prefetchtrees(
            c.manifestnode()
            for c in ctxs
            if (prefetch == AllTrees or c.phase() != phases.public)
            and c.manifestnode() != nullid
        )
    except shallowutil.MissingNodesError:
        # The server may not always have the manifests (like when we need to do
        # a conversion from a flat manifest to a tree), so eat it and let the
        # later fetch fail if necessary.
        # XXX: This doesn't work at all since the missing nodes error is UncategorizedNativeError now
        pass

    return result
