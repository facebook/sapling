# shallowbundle.py - bundle10 implementation for use with shallow repositories
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import fileserverclient, remotefilelog, shallowutil
import collections, os
from mercurial.node import bin, hex, nullid, nullrev
from mercurial import changegroup, revlog, phases, mdiff, match, bundlerepo
from mercurial import util
from mercurial.i18n import _

NoFiles = 0
LocalFiles = 1
AllFiles = 2

requirement = "remotefilelog"

def sortnodes(nodes, parentfunc):
    """Topologically sorts the nodes, using the parentfunc to find
    the parents of nodes."""
    nodes = set(nodes)
    childmap = {}
    parentmap = {}
    roots = []

    # Build a child and parent map
    for n in nodes:
        parents = [p for p in parentfunc(n) if p in nodes]
        parentmap[n] = set(parents)
        for p in parents:
            childmap.setdefault(p, set()).add(n)
        if not parents:
            roots.append(n)

    # Process roots, adding children to the queue as they become roots
    results = []
    while roots:
        n = roots.pop(0)
        results.append(n)
        if n in childmap:
            children = childmap[n]
            for c in children:
                childparents = parentmap[c]
                childparents.remove(n)
                if len(childparents) == 0:
                    # insert at the beginning, that way child nodes
                    # are likely to be output immediately after their
                    # parents.  This gives better compression results.
                    roots.insert(0, c)

    return results

def shallowgroup(cls, self, nodelist, rlog, lookup, units=None, reorder=None):
    if isinstance(rlog, revlog.revlog):
        for c in super(cls, self).group(nodelist, rlog, lookup,
                                        units=units):
            yield c
        return

    if len(nodelist) == 0:
        yield self.close()
        return

    nodelist = sortnodes(nodelist, rlog.parents)

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

    def generatefiles(self, changedfiles, linknodes, commonrevs, source):
        if requirement in self._repo.requirements:
            repo = self._repo
            if isinstance(repo, bundlerepo.bundlerepository):
                # If the bundle contains filelogs, we can't pull from it, since
                # bundlerepo is heavily tied to revlogs. Instead require that
                # the user use unbundle instead.
                # Force load the filelog data.
                bundlerepo.bundlerepository.file(repo, 'foo')
                if repo.bundlefilespos:
                    raise util.Abort("cannot pull from full bundles",
                                     hint="use `hg unbundle` instead")
                return []
            filestosend = self.shouldaddfilegroups(source)
            if filestosend == NoFiles:
                changedfiles = list([f for f in changedfiles if not repo.shallowmatch(f)])
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
                            if not os.path.exists(localpath) and repo.shallowmatch(fname):
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
                repo.ui.warn("pulling from a shallow repo\n")
                return AllFiles

        return NoFiles

    def prune(self, rlog, missing, commonrevs):
        if isinstance(rlog, revlog.revlog):
            return super(shallowcg1packer, self).prune(rlog, missing,
                commonrevs)

        repo = self._repo
        results = []
        for fnode in missing:
            fctx = repo.filectx(rlog.filename, fileid=fnode)
            if fctx.linkrev() not in commonrevs:
                results.append(fnode)
        return results

    def nodechunk(self, revlog, node, prev, linknode):
        prefix = ''
        if prev == nullrev:
            delta = revlog.revision(node)
            prefix = mdiff.trivialdiffheader(len(delta))
        else:
            delta = revlog.revdiff(prev, node)
        p1, p2 = revlog.parents(node)
        meta = self.builddeltaheader(node, p1, p2, prev, linknode)
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

def getchangegroup(orig, repo, source, heads=None, common=None, bundlecaps=None):
    if not requirement in repo.requirements:
        return orig(repo, source, heads=heads, common=common,
                    bundlecaps=bundlecaps)

    original = repo.shallowmatch
    try:
        # if serving, only send files the clients has patterns for
        if source == 'serve':
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
        return orig(repo, source, heads, common, bundlecaps)
    finally:
        repo.shallowmatch = original

def addchangegroupfiles(orig, repo, source, revmap, trp, pr, needfiles):
    if not requirement in repo.requirements:
        return orig(repo, source, revmap, trp, pr, needfiles)

    files = 0
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
        f = chunkdata["filename"]
        repo.ui.debug("adding %s revisions\n" % f)
        pr()

        if not repo.shallowmatch(f):
            fl = repo.file(f)
            fl.addgroup(source, revmap, trp)
            continue

        chain = None
        while True:
            revisiondata = source.deltachunk(chain)
            if not revisiondata:
                break

            chain = revisiondata['node']

            revisiondatas[(f, chain)] = revisiondata
            queue.append((f, chain))

            if f not in visited:
                files += 1
                visited.add(f)

        if chain == None:
            raise util.Abort(_("received file revlog group is empty"))

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
        dependents = [revisiondata['p1'], revisiondata['p2'],
                      revisiondata['deltabase']]

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
            raise util.Abort(_("circular node dependency"))

        fl = repo.file(f)

        revisiondata = revisiondatas[(f, node)]
        p1 = revisiondata['p1']
        p2 = revisiondata['p2']
        linknode = revisiondata['cs']
        deltabase = revisiondata['deltabase']
        delta = revisiondata['delta']

        if not available(f, node, f, deltabase):
            continue

        base = fl.revision(deltabase)
        text = mdiff.patch(base, delta)
        if isinstance(text, buffer):
            text = str(text)

        meta, text = remotefilelog._parsemeta(text)
        if 'copy' in meta:
            copyfrom = meta['copy']
            copynode = bin(meta['copyrev'])
            copyfl = repo.file(copyfrom)
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

    return len(revisiondatas), files
