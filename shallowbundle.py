# shallowbundle.py - revlog implementation where the content is stored remotely
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import hg
import fileserverclient
import collections, os
from mercurial.node import bin, hex, nullid, nullrev
from mercurial import changegroup, revlog
from mercurial.i18n import _

shallowremote = False

def shouldaddfilegroups(repo, source):
    isshallowclient = "shallowrepo" in repo.requirements
    if source == "push":
        return True
    if source == "serve":
        if isshallowclient:
            # commits in a shallow repo may not exist in the master server
            # so we need to return all the data on a pull
            ui.warn("pulling from a shallow repo\n")
            return True

        return not shallowremote

    return not isshallowclient

class shallowbundle(changegroup.bundle10):
    def generate(self, commonrevs, clnodes, fastpathlinkrev, source):
        if "shallowrepo" in self._repo.requirements:
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

        revmap = self._repo.changelog.rev
        nodelist = sorted(nodelist, key=lambda fnode: revmap(lookup(fnode)))

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

    def prune(self, rlog, missing, commonrevs, source):
        if isinstance(rlog, revlog.revlog):
            return super(shallowbundle, self).prune(rlog, missing,
                commonrevs, source)

        repo = self._repo
        if not shouldaddfilegroups(repo, source):
            return []
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
