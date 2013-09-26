# shallowbundle.py - bundle10 implementation for use with shallow repositories
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import fileserverclient
import collections, os
from mercurial.node import bin, hex, nullid, nullrev
from mercurial import changegroup, revlog, phases
from mercurial.i18n import _

shallowremote = False

NoFiles = 0
LocalFiles = 1
AllFiles = 2

def shouldaddfilegroups(repo, source):
    if not "remotefilelog" in repo.requirements:
        return AllFiles

    if source == "push":
        return AllFiles
    if source == "serve" or source == "pull":
        if shallowremote:
            return LocalFiles
        else:
            # Serving to a full repo requires us to serve everything
            repo.ui.warn("pulling from a shallow repo\n")
            return AllFiles

    return NoFiles

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

class shallowbundle(changegroup.bundle10):
    def generate(self, commonrevs, clnodes, fastpathlinkrev, source):
        if "remotefilelog" in self._repo.requirements:
            fastpathlinkrev = False

        return super(shallowbundle, self).generate(commonrevs, clnodes,
            fastpathlinkrev, source)

    def group(self, nodelist, rlog, lookup, units=None, reorder=None):
        if isinstance(rlog, revlog.revlog):
            for c in super(shallowbundle, self).group(nodelist, rlog, lookup,
                                                      units, reorder):
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

    def generatefiles(self, changedfiles, linknodes, commonrevs, source):
        if "remotefilelog" in self._repo.requirements:
            repo = self._repo
            filestosend = shouldaddfilegroups(repo, source)
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

                fileserverclient.client.prefetch(repo, files)

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

                fileserverclient.client.prefetch(repo, prevfiles)

        return super(shallowbundle, self).generatefiles(changedfiles,
                     linknodes, commonrevs, source)

    def prune(self, rlog, missing, commonrevs, source):
        if isinstance(rlog, revlog.revlog):
            return super(shallowbundle, self).prune(rlog, missing,
                commonrevs, source)

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
