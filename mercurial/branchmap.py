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
    filename = "cache/branch2"
    if repo.filtername:
        filename = '%s-%s' % (filename, repo.filtername)
    return filename

def read(repo):
    try:
        f = repo.opener(_filename(repo))
        lines = f.read().split('\n')
        f.close()
    except (IOError, OSError):
        return None

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
            node, state, label = l.split(" ", 2)
            if state not in 'oc':
                raise ValueError('invalid branch state')
            label = encoding.tolocal(label.strip())
            if not node in repo:
                raise ValueError('node %s does not exist' % node)
            node = bin(node)
            partial.setdefault(label, []).append(node)
            if state == 'c':
                partial._closednodes.add(node)
    except KeyboardInterrupt:
        raise
    except Exception, inst:
        if repo.ui.debugflag:
            msg = 'invalid branchheads cache'
            if repo.filtername is not None:
                msg += ' (%s)' % repo.filtername
            msg += ': %s\n'
            repo.ui.warn(msg % inst)
        partial = None
    return partial



### Nearest subset relation
# Nearest subset of filter X is a filter Y so that:
# * Y is included in X,
# * X - Y is as small as possible.
# This create and ordering used for branchmap purpose.
# the ordering may be partial
subsettable = {None: 'visible',
               'visible': 'served',
               'served': 'immutable',
               'immutable': 'base'}

def updatecache(repo):
    cl = repo.changelog
    filtername = repo.filtername
    partial = repo._branchcaches.get(filtername)

    revs = []
    if partial is None or not partial.validfor(repo):
        partial = read(repo)
        if partial is None:
            subsetname = subsettable.get(filtername)
            if subsetname is None:
                partial = branchcache()
            else:
                subset = repo.filtered(subsetname)
                partial = subset.branchmap().copy()
                extrarevs = subset.changelog.filteredrevs - cl.filteredrevs
                revs.extend(r for  r in extrarevs if r <= partial.tiprev)
    revs.extend(cl.revs(start=partial.tiprev + 1))
    if revs:
        partial.update(repo, revs)
        partial.write(repo)
    assert partial.validfor(repo), filtername
    repo._branchcaches[repo.filtername] = partial

class branchcache(dict):
    """A dict like object that hold branches heads cache.

    This cache is used to avoid costly computations to determine all the
    branch heads of a repo.

    The cache is serialized on disk in the following format:

    <tip hex node> <tip rev number> [optional filtered repo hex hash]
    <branch head hex node> <open/closed state> <branch name>
    <branch head hex node> <open/closed state> <branch name>
    ...

    The first line is used to check if the cache is still valid. If the
    branch cache is for a filtered repo view, an optional third hash is
    included that hashes the hashes of all filtered revisions.

    The open/closed state is represented by a single letter 'o' or 'c'.
    This field can be used to avoid changelog reads when determining if a
    branch head closes a branch or not.
    """

    def __init__(self, entries=(), tipnode=nullid, tiprev=nullrev,
                 filteredhash=None, closednodes=None):
        super(branchcache, self).__init__(entries)
        self.tipnode = tipnode
        self.tiprev = tiprev
        self.filteredhash = filteredhash
        # closednodes is a set of nodes that close their branch. If the branch
        # cache has been updated, it may contain nodes that are no longer
        # heads.
        if closednodes is None:
            self._closednodes = set()
        else:
            self._closednodes = closednodes

    def _hashfiltered(self, repo):
        """build hash of revision filtered in the current cache

        Tracking tipnode and tiprev is not enough to ensure validity of the
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
        """Is the cache content valid regarding a repo

        - False when cached tipnode is unknown or if we detect a strip.
        - True when cache is up to date or a subset of current repo."""
        try:
            return ((self.tipnode == repo.changelog.node(self.tiprev))
                    and (self.filteredhash == self._hashfiltered(repo)))
        except IndexError:
            return False

    def _branchtip(self, heads):
        tip = heads[-1]
        closed = True
        for h in reversed(heads):
            if h not in self._closednodes:
                tip = h
                closed = False
                break
        return tip, closed

    def branchtip(self, branch):
        return self._branchtip(self[branch])[0]

    def copy(self):
        """return an deep copy of the branchcache object"""
        return branchcache(self, self.tipnode, self.tiprev, self.filteredhash,
                           self._closednodes)

    def write(self, repo):
        try:
            f = repo.opener(_filename(repo), "w", atomictemp=True)
            cachekey = [hex(self.tipnode), str(self.tiprev)]
            if self.filteredhash is not None:
                cachekey.append(hex(self.filteredhash))
            f.write(" ".join(cachekey) + '\n')
            for label, nodes in sorted(self.iteritems()):
                for node in nodes:
                    if node in self._closednodes:
                        state = 'c'
                    else:
                        state = 'o'
                    f.write("%s %s %s\n" % (hex(node), state,
                                            encoding.fromlocal(label)))
            f.close()
        except (IOError, OSError, util.Abort):
            # Abort may be raise by read only opener
            pass

    def update(self, repo, revgen):
        """Given a branchhead cache, self, that may have extra nodes or be
        missing heads, and a generator of nodes that are at least a superset of
        heads missing, this function updates self to be correct.
        """
        cl = repo.changelog
        # collect new branch entries
        newbranches = {}
        getbranchinfo = cl.branchinfo
        for r in revgen:
            branch, closesbranch = getbranchinfo(r)
            node = cl.node(r)
            newbranches.setdefault(branch, []).append(node)
            if closesbranch:
                self._closednodes.add(node)
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

        if not self.validfor(repo):
            # cache key are not valid anymore
            self.tipnode = nullid
            self.tiprev = nullrev
            for heads in self.values():
                tiprev = max(cl.rev(node) for node in heads)
                if tiprev > self.tiprev:
                    self.tipnode = cl.node(tiprev)
                    self.tiprev = tiprev
        self.filteredhash = self._hashfiltered(repo)
