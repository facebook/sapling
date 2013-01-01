# branchmap.py - logic to computes, maintain and stores branchmap for local repo
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from node import bin, hex, nullid, nullrev
import encoding
import util

def _filename(repo):
    """name of a branchcache file for a given repo or repoview"""
    filename = "cache/branchheads"
    if repo.filtername:
        filename = '%s-%s' % (filename, repo.filtername)
    return filename

def read(repo):
    try:
        f = repo.opener(_filename(repo))
        lines = f.read().split('\n')
        f.close()
    except (IOError, OSError):
        return branchcache()

    try:
        cachekey = lines.pop(0).split(" ", 2)
        last, lrev = cachekey[:2]
        last, lrev = bin(last), int(lrev)
        filteredhash = None
        if len(cachekey) > 2:
            filteredhash = bin(cachekey[2])
        partial = branchcache(tipnode=last, tiprev=lrev,
                              filteredhash=filteredhash)
        if not partial.validfor(repo):
            # invalidate the cache
            raise ValueError('tip differs')
        for l in lines:
            if not l:
                continue
            node, label = l.split(" ", 1)
            label = encoding.tolocal(label.strip())
            if not node in repo:
                raise ValueError('node %s does not exist' % node)
            partial.setdefault(label, []).append(bin(node))
    except KeyboardInterrupt:
        raise
    except Exception, inst:
        if repo.ui.debugflag:
            msg = 'invalid branchheads cache'
            if repo.filtername is not None:
                msg += ' (%s)' % repo.filtername
            msg += ': %s\n'
            repo.ui.warn(msg % inst)
        partial = branchcache()
    return partial



def updatecache(repo):
    repo = repo.unfiltered()  # Until we get a smarter cache management
    cl = repo.changelog
    partial = repo._branchcache

    if partial is None or not partial.validfor(repo):
        partial = read(repo)

    catip = repo._cacheabletip()
    # if partial.tiprev == catip: cache is already up to date
    # if partial.tiprev >  catip: we have uncachable element in `partial` can't
    #                             write on disk
    if partial.tiprev < catip:
        ctxgen = (repo[r] for r in cl.revs(partial.tiprev + 1, catip))
        partial.update(repo, ctxgen)
        partial.write(repo)
    # If cacheable tip were lower than actual tip, we need to update the
    # cache up to tip. This update (from cacheable to actual tip) is not
    # written to disk since it's not cacheable.
    tiprev = cl.rev(cl.tip())
    if partial.tiprev < tiprev:
        ctxgen = (repo[r] for r in cl.revs(partial.tiprev + 1, tiprev))
        partial.update(repo, ctxgen)
    repo._branchcache = partial

class branchcache(dict):
    """A dict like object that hold branches heads cache"""

    def __init__(self, entries=(), tipnode=nullid, tiprev=nullrev,
                 filteredhash=None):
        super(branchcache, self).__init__(entries)
        self.tipnode = tipnode
        self.tiprev = tiprev
        self.filteredhash = filteredhash

    def _hashfiltered(self, repo):
        """build hash of revision filtered in the current cache

        Tracking tipnode and tiprev is not enough to ensure validaty of the
        cache as they do not help to distinct cache that ignored various
        revision bellow tiprev.

        To detect such difference, we build a cache of all ignored revisions.
        """
        cl = repo.changelog
        if not cl.filteredrevs:
            return None
        key = None
        revs = sorted(r for r in cl.filteredrevs if r <= self.tiprev)
        if revs:
            s = util.sha1()
            for rev in revs:
                s.update('%s;' % rev)
            key = s.digest()
        return key

    def validfor(self, repo):
        """Is the cache content valide regarding a repo

        - False when cached tipnode are unknown or if we detect a strip.
        - True when cache is up to date or a subset of current repo."""
        try:
            return ((self.tipnode == repo.changelog.node(self.tiprev))
                    and (self.filteredhash == self._hashfiltered(repo)))
        except IndexError:
            return False


    def write(self, repo):
        try:
            f = repo.opener(_filename(repo), "w", atomictemp=True)
            cachekey = [hex(self.tipnode), str(self.tiprev)]
            if self.filteredhash is not None:
                cachekey.append(hex(self.filteredhash))
            f.write(" ".join(cachekey) + '\n')
            for label, nodes in self.iteritems():
                for node in nodes:
                    f.write("%s %s\n" % (hex(node), encoding.fromlocal(label)))
            f.close()
        except (IOError, OSError):
            pass

    def update(self, repo, ctxgen):
        """Given a branchhead cache, self, that may have extra nodes or be
        missing heads, and a generator of nodes that are at least a superset of
        heads missing, this function updates self to be correct.
        """
        cl = repo.changelog
        # collect new branch entries
        newbranches = {}
        for c in ctxgen:
            newbranches.setdefault(c.branch(), []).append(c.node())
        # if older branchheads are reachable from new ones, they aren't
        # really branchheads. Note checking parents is insufficient:
        # 1 (branch a) -> 2 (branch b) -> 3 (branch a)
        for branch, newnodes in newbranches.iteritems():
            bheads = self.setdefault(branch, [])
            # Remove candidate heads that no longer are in the repo (e.g., as
            # the result of a strip that just happened).  Avoid using 'node in
            # self' here because that dives down into branchcache code somewhat
            # recursively.
            bheadrevs = [cl.rev(node) for node in bheads
                         if cl.hasnode(node)]
            newheadrevs = [cl.rev(node) for node in newnodes
                           if cl.hasnode(node)]
            ctxisnew = bheadrevs and min(newheadrevs) > max(bheadrevs)
            # Remove duplicates - nodes that are in newheadrevs and are already
            # in bheadrevs.  This can happen if you strip a node whose parent
            # was already a head (because they're on different branches).
            bheadrevs = sorted(set(bheadrevs).union(newheadrevs))

            # Starting from tip means fewer passes over reachable.  If we know
            # the new candidates are not ancestors of existing heads, we don't
            # have to examine ancestors of existing heads
            if ctxisnew:
                iterrevs = sorted(newheadrevs)
            else:
                iterrevs = list(bheadrevs)

            # This loop prunes out two kinds of heads - heads that are
            # superseded by a head in newheadrevs, and newheadrevs that are not
            # heads because an existing head is their descendant.
            while iterrevs:
                latest = iterrevs.pop()
                if latest not in bheadrevs:
                    continue
                ancestors = set(cl.ancestors([latest],
                                                         bheadrevs[0]))
                if ancestors:
                    bheadrevs = [b for b in bheadrevs if b not in ancestors]
            self[branch] = [cl.node(rev) for rev in bheadrevs]
            tiprev = max(bheadrevs)
            if tiprev > self.tiprev:
                self.tipnode = cl.node(tiprev)
                self.tiprev = tiprev

        # There may be branches that cease to exist when the last commit in the
        # branch was stripped.  This code filters them out.  Note that the
        # branch that ceased to exist may not be in newbranches because
        # newbranches is the set of candidate heads, which when you strip the
        # last commit in a branch will be the parent branch.
        droppednodes = []
        for branch in self.keys():
            nodes = [head for head in self[branch]
                     if cl.hasnode(head)]
            if not nodes:
                droppednodes.extend(nodes)
                del self[branch]
        if ((not self.validfor(repo)) or (self.tipnode in droppednodes)):

            # cache key are not valid anymore
            self.tipnode = nullid
            self.tiprev = nullrev
            for heads in self.values():
                tiprev = max(cl.rev(node) for node in heads)
                if tiprev > self.tiprev:
                    self.tipnode = cl.node(tiprev)
                    self.tiprev = tiprev
        self.filteredhash = self._hashfiltered(repo)
