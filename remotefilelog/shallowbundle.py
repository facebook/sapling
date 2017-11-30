# shallowbundle.py - bundle10 implementation for use with shallow repositories
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from . import fileserverclient, remotefilelog, shallowutil
import os
from mercurial.node import bin, hex, nullid
from mercurial import changegroup, mdiff, match, bundlerepo
from mercurial import util, error
from mercurial.i18n import _

NoFiles = 0
LocalFiles = 1
AllFiles = 2

requirement = "remotefilelog"

def shallowgroup(cls, self, nodelist, rlog, lookup, units=None, reorder=None):
    if not isinstance(rlog, remotefilelog.remotefilelog):
        for c in super(cls, self).group(nodelist, rlog, lookup,
                                        units=units):
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
        linknode = lookup(curr)
        for c in self.nodechunk(rlog, curr, prev, linknode):
            yield c

    yield self.close()

def _cansendflat(repo, mfnodes):
    if not util.safehasattr(repo.manifestlog, '_revlog'):
        return False

    if repo.ui.configbool('treemanifest', 'treeonly'):
        return False

    revlog = repo.manifestlog._revlog
    for mfnode in mfnodes:
        if mfnode not in revlog.nodemap:
            return False

    return True

@shallowutil.interposeclass(changegroup, 'cg1packer')
class shallowcg1packer(changegroup.cg1packer):
    def generate(self, commonrevs, clnodes, fastpathlinkrev, source):
        if "remotefilelog" in self._repo.requirements:
            fastpathlinkrev = False

        return super(shallowcg1packer, self).generate(commonrevs, clnodes,
            fastpathlinkrev, source)

    def group(self, nodelist, rlog, lookup, units=None, reorder=None):
        return shallowgroup(shallowcg1packer, self, nodelist, rlog, lookup,
                            units=units)

    def generatemanifests(self, commonrevs, clrevorder, fastpathlinkrev,
                          mfs, fnodes, *args, **kwargs):
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
        if _cansendflat(self._repo, mfs.keys()):
            # In this code path, generating the manifests populates fnodes for
            # us.
            chunks = super(shallowcg1packer, self).generatemanifests(
                commonrevs,
                clrevorder,
                fastpathlinkrev,
                mfs,
                fnodes,
                *args, **kwargs)
            for chunk in chunks:
                yield chunk
        else:
            # If not using the fast path, we need to discover what files to send
            if not fastpathlinkrev:
                mflog = self._repo.manifestlog
                for mfnode, clnode in mfs.iteritems():
                    mfctx = mflog[mfnode]
                    p1node = mfctx.parents[0]
                    p1ctx = mflog[p1node]
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
                bundlerepo.bundlerepository.file(repo, 'foo')
                if repo._cgfilespos:
                    raise error.Abort("cannot pull from full bundles",
                                      hint="use `hg unbundle` instead")
                return []
            filestosend = self.shouldaddfilegroups(source)
            if filestosend == NoFiles:
                changedfiles = list([f for f in changedfiles
                                     if not repo.shallowmatch(f)])
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
                            localkey = fileserverclient.getlocalkey(fname,
                                                                    hex(fnode))
                            localpath = repo.sjoin(os.path.join("data",
                                                                localkey))
                            if (not os.path.exists(localpath)
                                and repo.shallowmatch(fname)):
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

        return super(shallowcg1packer, self).generatefiles(changedfiles,
                     linknodes, commonrevs, source)

    def shouldaddfilegroups(self, source):
        repo = self._repo
        if not requirement in repo.requirements:
            return AllFiles

        if source == "push" or source == "bundle":
            return AllFiles

        caps = self._bundlecaps or []
        if source == "serve" or source == "pull":
            if 'remotefilelog' in caps:
                return LocalFiles
            else:
                # Serving to a full repo requires us to serve everything
                repo.ui.warn(_("pulling from a shallow repo\n"))
                return AllFiles

        return NoFiles

    def prune(self, rlog, missing, commonrevs):
        if not isinstance(rlog, remotefilelog.remotefilelog):
            return super(shallowcg1packer, self).prune(rlog, missing,
                commonrevs)

        repo = self._repo
        results = []
        for fnode in missing:
            fctx = repo.filectx(rlog.filename, fileid=fnode)
            if fctx.linkrev() not in commonrevs:
                results.append(fnode)
        return results

    def nodechunk(self, revlog, node, prevnode, linknode):
        prefix = ''
        if prevnode == nullid:
            delta = revlog.revision(node, raw=True)
            prefix = mdiff.trivialdiffheader(len(delta))
        else:
            # Actually uses remotefilelog.revdiff which works on nodes, not revs
            delta = revlog.revdiff(prevnode, node)
        p1, p2 = revlog.parents(node)
        flags = revlog.flags(node)
        meta = self.builddeltaheader(node, p1, p2, prevnode, linknode, flags)
        meta += prefix
        l = len(meta) + len(delta)
        yield changegroup.chunkheader(l)
        yield meta
        yield delta

if util.safehasattr(changegroup, 'cg2packer'):
    # Mercurial >= 3.3
    @shallowutil.interposeclass(changegroup, 'cg2packer')
    class shallowcg2packer(changegroup.cg2packer):
        def group(self, nodelist, rlog, lookup, units=None, reorder=None):
            # for revlogs, shallowgroup will be called twice in the same stack
            # -- once here, once up the inheritance hierarchy in
            # shallowcg1packer. That's fine though because for revlogs,
            # shallowgroup doesn't do anything on top of the usual group
            # function. If that assumption changes this will have to be
            # revisited.
            return shallowgroup(shallowcg2packer, self, nodelist, rlog, lookup,
                                units=units)

if util.safehasattr(changegroup, 'cg3packer'):
    @shallowutil.interposeclass(changegroup, 'cg3packer')
    class shallowcg3packer(changegroup.cg3packer):
        def generatemanifests(self, commonrevs, clrevorder, fastpathlinkrev,
                              mfs, fnodes, *args, **kwargs):
            chunks = super(shallowcg3packer, self).generatemanifests(
                commonrevs,
                clrevorder,
                fastpathlinkrev,
                mfs,
                fnodes,
                *args, **kwargs)
            for chunk in chunks:
                yield chunk

            # If we're not sending flat manifests, then the subclass
            # generatemanifests call did not add the appropriate closing chunk
            # for a changegroup3.
            if not _cansendflat(self._repo, mfs.keys()):
                yield self._manifestsdone()

# Unused except in older versions of Mercurial
def getchangegroup(orig, repo, source, outgoing, bundlecaps=None, version='01'):
    def origmakechangegroup(repo, outgoing, version, source):
        return orig(repo, source, outgoing, bundlecaps=bundlecaps,
                    version=version)

    return makechangegroup(origmakechangegroup, repo, outgoing, version, source)

def makechangegroup(orig, repo, outgoing, version, source, *args, **kwargs):
    if not requirement in repo.requirements:
        return orig(repo, outgoing, version, source, *args, **kwargs)

    original = repo.shallowmatch
    try:
        # if serving, only send files the clients has patterns for
        if source == 'serve':
            bundlecaps = kwargs.get('bundlecaps')
            includepattern = None
            excludepattern = None
            for cap in (bundlecaps or []):
                if cap.startswith("includepattern="):
                    raw = cap[len("includepattern="):]
                    if raw:
                        includepattern = raw.split('\0')
                elif cap.startswith("excludepattern="):
                    raw = cap[len("excludepattern="):]
                    if raw:
                        excludepattern = raw.split('\0')
            if includepattern or excludepattern:
                repo.shallowmatch = match.match(repo.root, '', None,
                    includepattern, excludepattern)
            else:
                repo.shallowmatch = match.always(repo.root, '')
        return orig(repo, outgoing, version, source, *args, **kwargs)
    finally:
        repo.shallowmatch = original

def addchangegroupfiles(orig, repo, source, revmap, trp, expectedfiles, *args):
    if not requirement in repo.requirements:
        return orig(repo, source, revmap, trp, expectedfiles, *args)

    files = 0
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
    while True:
        chunkdata = source.filelogheader()
        if not chunkdata:
            break
        files += 1
        f = chunkdata["filename"]
        repo.ui.debug("adding %s revisions\n" % f)
        repo.ui.progress(_('files'), files, total=expectedfiles)

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

        base = fl.revision(deltabase, raw=True)
        text = mdiff.patch(base, delta)
        if isinstance(text, buffer):
            text = str(text)

        meta, text = shallowutil.parsemeta(text)
        if 'copy' in meta:
            copyfrom = meta['copy']
            copynode = bin(meta['copyrev'])
            if not available(f, node, copyfrom, copynode):
                continue

        for p in [p1, p2]:
            if p != nullid:
                if not available(f, node, f, p):
                    continue

        fl.add(text, meta, trp, linknode, p1, p2)
        processed.add((f, node))
        skipcount = 0

    repo.ui.progress(_('files'), None)

    return len(revisiondatas), newfiles
