# shallowbundle.py - bundle10 implementation for use with shallow repositories
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
from __future__ import absolute_import

import os

from mercurial import (
    bundlerepo,
    changegroup,
    error,
    match,
    mdiff,
    phases,
    progress,
    util,
)
from mercurial.i18n import _
from mercurial.node import bin, hex, nullid

from . import fileserverclient, remotefilelog, shallowutil


NoFiles = NoTrees = 0
LocalFiles = LocalTrees = 1
AllFiles = AllTrees = 2

requirement = "remotefilelog"

try:
    xrange(0)
except NameError:
    xrange = range


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
    for i in xrange(len(nodelist) - 1):
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
    def generate(self, commonrevs, clnodes, fastpathlinkrev, source):
        if "remotefilelog" in self._repo.requirements:
            fastpathlinkrev = False

        return super(shallowcg1packer, self).generate(
            commonrevs, clnodes, fastpathlinkrev, source
        )

    def group(self, nodelist, rlog, lookup, prog=None, reorder=None):
        return shallowgroup(shallowcg1packer, self, nodelist, rlog, lookup, prog=prog)

    def _cansendflat(self, mfnodes):
        repo = self._repo
        if "treeonly" in self._bundlecaps:
            return False

        if not util.safehasattr(repo.manifestlog, "_revlog"):
            return False

        if repo.ui.configbool("treemanifest", "treeonly"):
            return False

        revlog = repo.manifestlog._revlog
        for mfnode in mfnodes:
            if mfnode not in revlog.nodemap:
                return False

        return True

    def generatemanifests(
        self, commonrevs, clrevorder, fastpathlinkrev, mfs, fnodes, source
    ):
        """
        - `commonrevs` is the set of known commits on both sides
        - `clrevorder` is a mapping from cl node to rev number, used for
                       determining which commit is newer.
        - `mfs` is the potential manifest nodes to send,
                with maps to their linknodes
                { manifest root node -> link node }
        - `fnodes` is a mapping of { filepath -> { node -> clnode } }
                If fastpathlinkrev is false, we are responsible for populating
                fnodes.
        - `args` and `kwargs` are extra arguments that will be passed to the
                core generatemanifests method, whose length depends on the
                version of core Hg.
        """
        # If we're not using the fastpath, then all the trees will be necessary
        # so we can inspect which files changed and need to be sent. So let's
        # bulk fetch the trees up front.
        repo = self._repo
        sendtrees = NoTrees
        if not fastpathlinkrev and util.safehasattr(repo, "prefetchtrees"):
            sendtrees = cansendtrees(
                repo,
                mfs.values(),
                source=source,
                bundlecaps=self._bundlecaps,
                b2caps=self._b2caps,
            )

        if self._cansendflat(mfs.keys()):
            # In this code path, generating the manifests populates fnodes for
            # us.
            chunks = super(shallowcg1packer, self).generatemanifests(
                commonrevs, clrevorder, fastpathlinkrev, mfs, fnodes, source
            )
            for chunk in chunks:
                yield chunk
        else:
            # If not using the fast path, we need to discover what files to send
            if not fastpathlinkrev:
                # If we're sending files, we need to process the manifests
                filestosend = self.shouldaddfilegroups(source)
                if filestosend is not NoFiles:
                    mflog = repo.manifestlog
                    for mfnode, clnode in mfs.iteritems():
                        if sendtrees == LocalTrees:
                            # Don't inspect public commits, since we won't be
                            # sending them.
                            ctx = repo[clnode]
                            if ctx.phase() == phases.public:
                                continue

                        try:
                            mfctx = mflog[mfnode]
                            p1node = mfctx.parents[0]
                            p1ctx = mflog[p1node]
                        except LookupError:
                            if not repo.svfs.treemanifestserver:
                                raise
                            # If we can't find the flat version, look for trees
                            tmfl = mflog.treemanifestlog
                            mfctx = tmfl[mfnode]
                            p1node = tmfl[mfnode].parents[0]
                            p1ctx = tmfl[p1node]

                        diff = p1ctx.read().diff(mfctx.read()).iteritems()
                        for filename, ((anode, aflag), (bnode, bflag)) in diff:
                            if bnode is not None:
                                fclnodes = fnodes.setdefault(filename, {})
                                fclnode = fclnodes.setdefault(bnode, clnode)
                                if clrevorder[clnode] < clrevorder[fclnode]:
                                    fclnodes[bnode] = clnode

            yield self.close()

    def generatefiles(self, changedfiles, linknodes, commonrevs, source):
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
                        hint="use `hg unbundle` instead",
                    )
                return []
            filestosend = self.shouldaddfilegroups(source)
            if filestosend == NoFiles:
                changedfiles = list(
                    [f for f in changedfiles if not repo.shallowmatch(f)]
                )
            else:
                files = []
                # Prefetch the revisions being bundled
                for i, fname in enumerate(sorted(changedfiles)):
                    filerevlog = repo.file(fname)
                    linkrevnodes = linknodes(filerevlog, fname)
                    # Normally we'd prune the linkrevnodes first,
                    # but that would perform the server fetches one by one.
                    for fnode, cnode in list(linkrevnodes.iteritems()):
                        # Adjust linknodes so remote file revisions aren't sent
                        if filestosend == LocalFiles:
                            localkey = fileserverclient.getlocalkey(fname, hex(fnode))
                            localpath = repo.sjoin(os.path.join("data", localkey))
                            if not os.path.exists(localpath) and repo.shallowmatch(
                                fname
                            ):
                                del linkrevnodes[fnode]
                            else:
                                files.append((fname, hex(fnode)))
                        else:
                            files.append((fname, hex(fnode)))

                repo.fileservice.prefetch(files)

                # Prefetch the revisions that are going to be diffed against
                prevfiles = []
                for fname, fnode in files:
                    if repo.shallowmatch(fname):
                        fnode = bin(fnode)
                        filerevlog = repo.file(fname)
                        ancestormap = filerevlog.ancestormap(fnode)
                        p1, p2, linknode, copyfrom = ancestormap[fnode]
                        if p1 != nullid:
                            prevfiles.append((copyfrom or fname, hex(p1)))

                repo.fileservice.prefetch(prevfiles)

        return super(shallowcg1packer, self).generatefiles(
            changedfiles, linknodes, commonrevs, source
        )

    def shouldaddfilegroups(self, source):
        repo = self._repo
        if not requirement in repo.requirements:
            return AllFiles

        if source == "push" or source == "bundle":
            return AllFiles

        caps = self._bundlecaps or []
        if source == "serve" or source == "pull":
            if "remotefilelog" in caps:
                return LocalFiles
            else:
                # Serving to a full repo requires us to serve everything
                repo.ui.warn(_("pulling from a shallow repo\n"))
                return AllFiles

        return NoFiles

    def prune(self, rlog, missing, commonrevs):
        if not isinstance(rlog, remotefilelog.remotefilelog):
            return super(shallowcg1packer, self).prune(rlog, missing, commonrevs)

        repo = self._repo
        results = []
        for fnode in missing:
            fctx = repo.filectx(rlog.filename, fileid=fnode)
            if fctx.linkrev() not in commonrevs:
                results.append(fnode)
        return results

    def nodechunk(self, revlog, node, prevnode, linknode):
        prefix = ""
        if prevnode is not nullid and not revlog.candelta(prevnode, node):
            basenode = nullid
        else:
            basenode = prevnode
        if basenode == nullid:
            delta = revlog.revision(node, raw=True)
            prefix = mdiff.trivialdiffheader(len(delta))
        else:
            # Actually uses remotefilelog.revdiff which works on nodes, not revs
            delta = revlog.revdiff(basenode, node)
        p1, p2 = revlog.parents(node)
        flags = revlog.flags(node)
        meta = self.builddeltaheader(node, p1, p2, basenode, linknode, flags)
        meta += prefix
        l = len(meta) + len(delta)
        yield changegroup.chunkheader(l)
        yield meta
        yield delta


if util.safehasattr(changegroup, "cg2packer"):
    # Mercurial >= 3.3
    @shallowutil.interposeclass(changegroup, "cg2packer")
    class shallowcg2packer(changegroup.cg2packer):
        def group(self, nodelist, rlog, lookup, prog=None, reorder=None):
            # for revlogs, shallowgroup will be called twice in the same stack
            # -- once here, once up the inheritance hierarchy in
            # shallowcg1packer. That's fine though because for revlogs,
            # shallowgroup doesn't do anything on top of the usual group
            # function. If that assumption changes this will have to be
            # revisited.
            return shallowgroup(
                shallowcg2packer, self, nodelist, rlog, lookup, prog=prog
            )


if util.safehasattr(changegroup, "cg3packer"):

    @shallowutil.interposeclass(changegroup, "cg3packer")
    class shallowcg3packer(changegroup.cg3packer):
        def generatemanifests(
            self, commonrevs, clrevorder, fastpathlinkrev, mfs, fnodes, *args, **kwargs
        ):
            chunks = super(shallowcg3packer, self).generatemanifests(
                commonrevs, clrevorder, fastpathlinkrev, mfs, fnodes, *args, **kwargs
            )
            for chunk in chunks:
                yield chunk

            # If we're not sending flat manifests, then the subclass
            # generatemanifests call did not add the appropriate closing chunk
            # for a changegroup3.
            if not self._cansendflat(mfs.keys()):
                yield self._manifestsdone()


# Unused except in older versions of Mercurial
def getchangegroup(orig, repo, source, outgoing, bundlecaps=None, version="01"):
    def origmakechangegroup(repo, outgoing, version, source):
        return orig(repo, source, outgoing, bundlecaps=bundlecaps, version=version)

    return makechangegroup(origmakechangegroup, repo, outgoing, version, source)


def makechangegroup(orig, repo, outgoing, version, source, *args, **kwargs):
    if not requirement in repo.requirements:
        return orig(repo, outgoing, version, source, *args, **kwargs)

    original = repo.shallowmatch
    try:
        # if serving, only send files the clients has patterns for
        if source == "serve":
            bundlecaps = kwargs.get("bundlecaps")
            includepattern = None
            excludepattern = None
            for cap in bundlecaps or []:
                if cap.startswith("includepattern="):
                    raw = cap[len("includepattern=") :]
                    if raw:
                        includepattern = raw.split("\0")
                elif cap.startswith("excludepattern="):
                    raw = cap[len("excludepattern=") :]
                    if raw:
                        excludepattern = raw.split("\0")
            if includepattern or excludepattern:
                repo.shallowmatch = match.match(
                    repo.root, "", None, includepattern, excludepattern
                )
            else:
                repo.shallowmatch = match.always(repo.root, "")
        return orig(repo, outgoing, version, source, *args, **kwargs)
    finally:
        repo.shallowmatch = original


def addchangegroupfiles(orig, repo, source, revmap, trp, expectedfiles, *args):
    if not requirement in repo.requirements:
        return orig(repo, source, revmap, trp, expectedfiles, *args)

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
    with progress.bar(repo.ui, _("files"), total=expectedfiles) as prog:
        while True:
            chunkdata = source.filelogheader()
            if not chunkdata:
                break
            f = chunkdata["filename"]
            repo.ui.debug("adding %s revisions\n" % f)
            prog.value += 1

            if not repo.shallowmatch(f):
                fl = repo.file(f)
                deltas = source.deltaiter()
                fl.addgroup(deltas, revmap, trp)
                continue

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
            prefetchfiles.append((f, hex(dependent)))

    repo.fileservice.prefetch(prefetchfiles)

    # Apply the revisions in topological order such that a revision
    # is only written once it's deltabase and parents have been written.
    while queue:
        f, node = queue.pop(0)
        if (f, node) in processed:
            continue

        skipcount += 1
        if skipcount > len(queue) + 1:
            raise error.Abort(_("circular node dependency"))

        fl = repo.file(f)

        revisiondata = revisiondatas[(f, node)]
        # revisiondata: (node, p1, p2, cs, deltabase, delta, flags)
        node, p1, p2, linknode, deltabase, delta, flags = revisiondata

        if not available(f, node, f, deltabase):
            continue

        # Deltas are always against flags=0 rawtext (see revdiff and its
        # callers), if deltabase is not nullid.
        if flags and deltabase != nullid:
            raise error.Abort("unexpected deltabase")

        # If deltabase does not have flags=0, convert it to flags=0
        # rawtext, which is equivalent to raw=False text.
        #
        # This happens if a non-LFS delta is being applied to a LFS base.
        base = fl.revision(deltabase, raw=False)

        # For LFS pointer (rawtext), delta contains flags!=0 rawtext. So
        # "rawtext" will be the original LFS rawtext, and base should be
        # an empty string in this case.
        rawtext = mdiff.patch(base, delta)
        if isinstance(rawtext, buffer):  # noqa
            rawtext = bytes(rawtext)

        meta, text = shallowutil.parsemeta(rawtext, flags)
        if "copy" in meta:
            copyfrom = meta["copy"]
            copynode = bin(meta["copyrev"])
            if not available(f, node, copyfrom, copynode):
                continue

        for p in [p1, p2]:
            if p != nullid:
                if not available(f, node, f, p):
                    continue

        # Use addrawrevision so if it's already LFS, take it as-is, do not
        # re-calculate the LFS object.
        fl.addrawrevision(rawtext, trp, linknode, p1, p2, node=node, flags=flags)
        processed.add((f, node))
        skipcount = 0

    return len(revisiondatas), newfiles


def cansendtrees(repo, nodes, source=None, bundlecaps=None, b2caps=None):
    """Sending trees has the following rules:

    Clients:
    - If sendtrees is False, send no trees
    - else send draft trees

    Server:
    - Do not send trees unless it's an infinitepush.

    The function also does a prefetch on clients, so all the necessary trees are
    bulk downloaded.
    """

    if b2caps is None:
        b2caps = {}
    if bundlecaps is None:
        bundlecaps = set()
    sendtrees = repo.ui.configbool("treemanifest", "sendtrees")
    treeonly = repo.ui.configbool("treemanifest", "treeonly")

    result = AllTrees
    prefetch = AllTrees

    if repo.svfs.treemanifestserver:
        if source == "infinitepushpull" and "True" in b2caps.get("treemanifest", []):
            result = AllTrees
        else:
            result = NoTrees
        return result

    # Else, is a client
    if not sendtrees:
        result = NoTrees
        # If we're not in treeonly mode, we will consult the manifests when
        # getting ready to send the flat manifests. This will cause tree
        # manifest lookups, so let's go ahead and bulk prefetch them.
        prefetch = AllTrees
    elif "treemanifestserver" in bundlecaps or "True" in b2caps.get(
        "treemanifestserver", []
    ):
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
        if not treeonly:
            # If we're sending trees and flats, then we need to prefetch
            # everything, since when it inspects the flat manifests it will
            # attempt to access the tree equivalent.
            prefetch = AllTrees

    ctxs = [repo[node] for node in nodes]

    try:
        repo.prefetchtrees(
            c.manifestnode()
            for c in ctxs
            if prefetch == AllTrees or c.phase() != phases.public
        )
    except shallowutil.MissingNodesError:
        # The server may not always have the manifests (like when we need to do
        # a conversion from a flat manifest to a tree), so eat it and let the
        # later fetch fail if necessary.
        pass

    return result
