# branchmap.py - logic to computes, maintain and stores branchmap for local repo
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from node import bin, hex, nullid, nullrev
import encoding
import scmutil
import util
import time
from array import array
from struct import calcsize, pack, unpack

def _filename(repo):
    """name of a branchcache file for a given repo or repoview"""
    filename = "cache/branch2"
    if repo.filtername:
        filename = '%s-%s' % (filename, repo.filtername)
    return filename

def read(repo):
    try:
        f = repo.vfs(_filename(repo))
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
    except Exception as inst:
        if repo.ui.debugflag:
            msg = 'invalid branchheads cache'
            if repo.filtername is not None:
                msg += ' (%s)' % repo.filtername
            msg += ': %s\n'
            repo.ui.debug(msg % inst)
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

    def validfor(self, repo):
        """Is the cache content valid regarding a repo

        - False when cached tipnode is unknown or if we detect a strip.
        - True when cache is up to date or a subset of current repo."""
        try:
            return ((self.tipnode == repo.changelog.node(self.tiprev))
                    and (self.filteredhash == \
                         scmutil.filteredhash(repo, self.tiprev)))
        except IndexError:
            return False

    def _branchtip(self, heads):
        '''Return tuple with last open head in heads and false,
        otherwise return last closed head and true.'''
        tip = heads[-1]
        closed = True
        for h in reversed(heads):
            if h not in self._closednodes:
                tip = h
                closed = False
                break
        return tip, closed

    def branchtip(self, branch):
        '''Return the tipmost open head on branch head, otherwise return the
        tipmost closed head on branch.
        Raise KeyError for unknown branch.'''
        return self._branchtip(self[branch])[0]

    def branchheads(self, branch, closed=False):
        heads = self[branch]
        if not closed:
            heads = [h for h in heads if h not in self._closednodes]
        return heads

    def iterbranches(self):
        for bn, heads in self.iteritems():
            yield (bn, heads) + self._branchtip(heads)

    def copy(self):
        """return an deep copy of the branchcache object"""
        return branchcache(self, self.tipnode, self.tiprev, self.filteredhash,
                           self._closednodes)

    def write(self, repo):
        try:
            f = repo.vfs(_filename(repo), "w", atomictemp=True)
            cachekey = [hex(self.tipnode), str(self.tiprev)]
            if self.filteredhash is not None:
                cachekey.append(hex(self.filteredhash))
            f.write(" ".join(cachekey) + '\n')
            nodecount = 0
            for label, nodes in sorted(self.iteritems()):
                for node in nodes:
                    nodecount += 1
                    if node in self._closednodes:
                        state = 'c'
                    else:
                        state = 'o'
                    f.write("%s %s %s\n" % (hex(node), state,
                                            encoding.fromlocal(label)))
            f.close()
            repo.ui.log('branchcache',
                        'wrote %s branch cache with %d labels and %d nodes\n',
                        repo.filtername, len(self), nodecount)
        except (IOError, OSError, util.Abort) as inst:
            repo.ui.debug("couldn't write branch cache: %s\n" % inst)
            # Abort may be raise by read only opener
            pass

    def update(self, repo, revgen):
        """Given a branchhead cache, self, that may have extra nodes or be
        missing heads, and a generator of nodes that are strictly a superset of
        heads missing, this function updates self to be correct.
        """
        starttime = time.time()
        cl = repo.changelog
        # collect new branch entries
        newbranches = {}
        getbranchinfo = repo.revbranchcache().branchinfo
        for r in revgen:
            branch, closesbranch = getbranchinfo(r)
            newbranches.setdefault(branch, []).append(r)
            if closesbranch:
                self._closednodes.add(cl.node(r))

        # fetch current topological heads to speed up filtering
        topoheads = set(cl.headrevs())

        # if older branchheads are reachable from new ones, they aren't
        # really branchheads. Note checking parents is insufficient:
        # 1 (branch a) -> 2 (branch b) -> 3 (branch a)
        for branch, newheadrevs in newbranches.iteritems():
            bheads = self.setdefault(branch, [])
            bheadset = set(cl.rev(node) for node in bheads)

            # This have been tested True on all internal usage of this function.
            # run it again in case of doubt
            # assert not (set(bheadrevs) & set(newheadrevs))
            newheadrevs.sort()
            bheadset.update(newheadrevs)

            # This prunes out two kinds of heads - heads that are superseded by
            # a head in newheadrevs, and newheadrevs that are not heads because
            # an existing head is their descendant.
            uncertain = bheadset - topoheads
            if uncertain:
                floorrev = min(uncertain)
                ancestors = set(cl.ancestors(newheadrevs, floorrev))
                bheadset -= ancestors
            bheadrevs = sorted(bheadset)
            self[branch] = [cl.node(rev) for rev in bheadrevs]
            tiprev = bheadrevs[-1]
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
        self.filteredhash = scmutil.filteredhash(repo, self.tiprev)

        duration = time.time() - starttime
        repo.ui.log('branchcache', 'updated %s branch cache in %.4f seconds\n',
                    repo.filtername, duration)

# Revision branch info cache

_rbcversion = '-v1'
_rbcnames = 'cache/rbc-names' + _rbcversion
_rbcrevs = 'cache/rbc-revs' + _rbcversion
# [4 byte hash prefix][4 byte branch name number with sign bit indicating open]
_rbcrecfmt = '>4sI'
_rbcrecsize = calcsize(_rbcrecfmt)
_rbcnodelen = 4
_rbcbranchidxmask = 0x7fffffff
_rbccloseflag = 0x80000000

class revbranchcache(object):
    """Persistent cache, mapping from revision number to branch name and close.
    This is a low level cache, independent of filtering.

    Branch names are stored in rbc-names in internal encoding separated by 0.
    rbc-names is append-only, and each branch name is only stored once and will
    thus have a unique index.

    The branch info for each revision is stored in rbc-revs as constant size
    records. The whole file is read into memory, but it is only 'parsed' on
    demand. The file is usually append-only but will be truncated if repo
    modification is detected.
    The record for each revision contains the first 4 bytes of the
    corresponding node hash, and the record is only used if it still matches.
    Even a completely trashed rbc-revs fill thus still give the right result
    while converging towards full recovery ... assuming no incorrectly matching
    node hashes.
    The record also contains 4 bytes where 31 bits contains the index of the
    branch and the last bit indicate that it is a branch close commit.
    The usage pattern for rbc-revs is thus somewhat similar to 00changelog.i
    and will grow with it but be 1/8th of its size.
    """

    def __init__(self, repo, readonly=True):
        assert repo.filtername is None
        self._repo = repo
        self._names = [] # branch names in local encoding with static index
        self._rbcrevs = array('c') # structs of type _rbcrecfmt
        self._rbcsnameslen = 0
        try:
            bndata = repo.vfs.read(_rbcnames)
            self._rbcsnameslen = len(bndata) # for verification before writing
            self._names = [encoding.tolocal(bn) for bn in bndata.split('\0')]
        except (IOError, OSError) as inst:
            if readonly:
                # don't try to use cache - fall back to the slow path
                self.branchinfo = self._branchinfo

        if self._names:
            try:
                data = repo.vfs.read(_rbcrevs)
                self._rbcrevs.fromstring(data)
            except (IOError, OSError) as inst:
                repo.ui.debug("couldn't read revision branch cache: %s\n" %
                              inst)
        # remember number of good records on disk
        self._rbcrevslen = min(len(self._rbcrevs) // _rbcrecsize,
                               len(repo.changelog))
        if self._rbcrevslen == 0:
            self._names = []
        self._rbcnamescount = len(self._names) # number of good names on disk
        self._namesreverse = dict((b, r) for r, b in enumerate(self._names))

    def branchinfo(self, rev):
        """Return branch name and close flag for rev, using and updating
        persistent cache."""
        changelog = self._repo.changelog
        rbcrevidx = rev * _rbcrecsize

        # avoid negative index, changelog.read(nullrev) is fast without cache
        if rev == nullrev:
            return changelog.branchinfo(rev)

        # if requested rev is missing, add and populate all missing revs
        if len(self._rbcrevs) < rbcrevidx + _rbcrecsize:
            self._rbcrevs.extend('\0' * (len(changelog) * _rbcrecsize -
                                         len(self._rbcrevs)))

        # fast path: extract data from cache, use it if node is matching
        reponode = changelog.node(rev)[:_rbcnodelen]
        cachenode, branchidx = unpack(
            _rbcrecfmt, buffer(self._rbcrevs, rbcrevidx, _rbcrecsize))
        close = bool(branchidx & _rbccloseflag)
        if close:
            branchidx &= _rbcbranchidxmask
        if cachenode == '\0\0\0\0':
            pass
        elif cachenode == reponode:
            return self._names[branchidx], close
        else:
            # rev/node map has changed, invalidate the cache from here up
            truncate = rbcrevidx + _rbcrecsize
            del self._rbcrevs[truncate:]
            self._rbcrevslen = min(self._rbcrevslen, truncate)

        # fall back to slow path and make sure it will be written to disk
        return self._branchinfo(rev)

    def _branchinfo(self, rev):
        """Retrieve branch info from changelog and update _rbcrevs"""
        changelog = self._repo.changelog
        b, close = changelog.branchinfo(rev)
        if b in self._namesreverse:
            branchidx = self._namesreverse[b]
        else:
            branchidx = len(self._names)
            self._names.append(b)
            self._namesreverse[b] = branchidx
        reponode = changelog.node(rev)
        if close:
            branchidx |= _rbccloseflag
        self._setcachedata(rev, reponode, branchidx)
        return b, close

    def _setcachedata(self, rev, node, branchidx):
        """Writes the node's branch data to the in-memory cache data."""
        rbcrevidx = rev * _rbcrecsize
        rec = array('c')
        rec.fromstring(pack(_rbcrecfmt, node, branchidx))
        self._rbcrevs[rbcrevidx:rbcrevidx + _rbcrecsize] = rec
        self._rbcrevslen = min(self._rbcrevslen, rev)

        tr = self._repo.currenttransaction()
        if tr:
            tr.addfinalize('write-revbranchcache', self.write)

    def write(self, tr=None):
        """Save branch cache if it is dirty."""
        repo = self._repo
        if self._rbcnamescount < len(self._names):
            try:
                if self._rbcnamescount != 0:
                    f = repo.vfs.open(_rbcnames, 'ab')
                    if f.tell() == self._rbcsnameslen:
                        f.write('\0')
                    else:
                        f.close()
                        repo.ui.debug("%s changed - rewriting it\n" % _rbcnames)
                        self._rbcnamescount = 0
                        self._rbcrevslen = 0
                if self._rbcnamescount == 0:
                    f = repo.vfs.open(_rbcnames, 'wb')
                f.write('\0'.join(encoding.fromlocal(b)
                                  for b in self._names[self._rbcnamescount:]))
                self._rbcsnameslen = f.tell()
                f.close()
            except (IOError, OSError, util.Abort) as inst:
                repo.ui.debug("couldn't write revision branch cache names: "
                              "%s\n" % inst)
                return
            self._rbcnamescount = len(self._names)

        start = self._rbcrevslen * _rbcrecsize
        if start != len(self._rbcrevs):
            revs = min(len(repo.changelog), len(self._rbcrevs) // _rbcrecsize)
            try:
                f = repo.vfs.open(_rbcrevs, 'ab')
                if f.tell() != start:
                    repo.ui.debug("truncating %s to %s\n" % (_rbcrevs, start))
                    f.seek(start)
                    f.truncate()
                end = revs * _rbcrecsize
                f.write(self._rbcrevs[start:end])
                f.close()
            except (IOError, OSError, util.Abort) as inst:
                repo.ui.debug("couldn't write revision branch cache: %s\n" %
                              inst)
                return
            self._rbcrevslen = revs
