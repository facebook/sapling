# branchmap.py - logic to computes, maintain and stores branchmap for local repo
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import struct

from . import encoding, error, scmutil, util
from .node import bin, hex, nullid, nullrev


calcsize = struct.calcsize
pack_into = struct.pack_into
unpack_from = struct.unpack_from


def _filename(repo):
    """name of a branchcache file for a given repo or repoview"""
    filename = "branch2"
    if repo.filtername:
        filename = "%s-%s" % (filename, repo.filtername)
    return filename


def read(repo):
    # Don't bother reading branchmap since branchcache.update() will be called
    # anyway and that is O(changelog).
    return None


### Nearest subset relation
# Nearest subset of filter X is a filter Y so that:
# * Y is included in X,
# * X - Y is as small as possible.
# This create and ordering used for branchmap purpose.
# the ordering may be partial
subsettable = {
    None: "visible",
    "visible": "served",
    "served": "immutable",
    "immutable": "base",
}


def updatecache(repo):
    # Don't write the branchmap if it's disabled.
    # The original logic has unnecessary steps, ex. it calculates the "served"
    # repoview as an attempt to build branchcache for "visible". And then
    # calculates "immutable" for calculating "served", recursively.
    #
    # Just use a shortcut path that construct the branchcache directly.
    partial = repo._branchcaches.get(repo.filtername)
    if partial is None:
        partial = branchcache()
    partial.update(repo, None)
    repo._branchcaches[repo.filtername] = partial


def replacecache(repo, bm):
    """Replace the branchmap cache for a repo with a branch mapping.

    This is likely only called during clone with a branch map from a remote.
    """
    # Don't write the branchmap if it's disabled.
    return


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

    def __init__(
        self,
        entries=(),
        tipnode=nullid,
        tiprev=nullrev,
        filteredhash=None,
        closednodes=None,
    ):
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
            return (self.tipnode == repo.changelog.node(self.tiprev)) and (
                self.filteredhash == scmutil.filteredhash(repo, self.tiprev)
            )
        except IndexError:
            return False

    def _branchtip(self, heads):
        """Return tuple with last open head in heads and false,
        otherwise return last closed head and true."""
        tip = heads[-1]
        closed = True
        for h in reversed(heads):
            if h not in self._closednodes:
                tip = h
                closed = False
                break
        return tip, closed

    def branchtip(self, branch):
        """Return the tipmost open head on branch head, otherwise return the
        tipmost closed head on branch.
        Raise KeyError for unknown branch."""
        return self._branchtip(self[branch])[0]

    def iteropen(self, nodes):
        return (n for n in nodes if n not in self._closednodes)

    def branchheads(self, branch, closed=False):
        heads = self[branch]
        if not closed:
            heads = list(self.iteropen(heads))
        return heads

    def iterbranches(self):
        for bn, heads in self.iteritems():
            yield (bn, heads) + self._branchtip(heads)

    def copy(self):
        """return an deep copy of the branchcache object"""
        return branchcache(
            self, self.tipnode, self.tiprev, self.filteredhash, self._closednodes
        )

    def write(self, repo):
        # Don't bother writing the branchcache if it's disabled.
        return None

    def update(self, repo, revgen):
        """Given a branchhead cache, self, that may have extra nodes or be
        missing heads, and a generator of nodes that are strictly a superset of
        heads missing, this function updates self to be correct.
        """
        # Behave differently if the cache is disabled.
        cl = repo.changelog
        tonode = cl.node

        if self.tiprev == len(cl) - 1 and self.validfor(repo):
            return

        # Since we have no branches, the default branch heads are equal to
        # cl.headrevs(). Note: cl.headrevs() is already sorted and it may return
        # -1.
        branchheads = [i for i in cl.headrevs() if i >= 0]

        if not branchheads:
            if "default" in self:
                del self["default"]
            tiprev = -1
        else:
            self["default"] = [tonode(rev) for rev in branchheads]
            tiprev = branchheads[-1]
        self.tipnode = cl.node(tiprev)
        self.tiprev = tiprev
        self.filteredhash = scmutil.filteredhash(repo, self.tiprev)
        repo.ui.log(
            "branchcache", "perftweaks updated %s branch cache\n", repo.filtername
        )


# Revision branch info cache

_rbcversion = "-v1"
_rbcnames = "rbc-names" + _rbcversion
_rbcrevs = "rbc-revs" + _rbcversion
# [4 byte hash prefix][4 byte branch name number with sign bit indicating open]
_rbcrecfmt = ">4sI"
_rbcrecsize = calcsize(_rbcrecfmt)
_rbcnodelen = 4
_rbcbranchidxmask = 0x7FFFFFFF
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
        self._names = []  # branch names in local encoding with static index
        self._rbcrevs = bytearray()
        self._rbcsnameslen = 0  # length of names read at _rbcsnameslen
        try:
            bndata = repo.cachevfs.read(_rbcnames)
            self._rbcsnameslen = len(bndata)  # for verification before writing
            if bndata:
                self._names = [encoding.tolocal(bn) for bn in bndata.split("\0")]
        except (IOError, OSError):
            if readonly:
                # don't try to use cache - fall back to the slow path
                self.branchinfo = self._branchinfo

        if self._names:
            try:
                data = repo.cachevfs.read(_rbcrevs)
                self._rbcrevs[:] = data
            except (IOError, OSError) as inst:
                repo.ui.debug("couldn't read revision branch cache: %s\n" % inst)
        # remember number of good records on disk
        self._rbcrevslen = min(len(self._rbcrevs) // _rbcrecsize, len(repo.changelog))
        if self._rbcrevslen == 0:
            self._names = []
        self._rbcnamescount = len(self._names)  # number of names read at
        # _rbcsnameslen
        self._namesreverse = dict((b, r) for r, b in enumerate(self._names))

    def _clear(self):
        self._rbcsnameslen = 0
        del self._names[:]
        self._rbcnamescount = 0
        self._namesreverse.clear()
        self._rbcrevslen = len(self._repo.changelog)
        self._rbcrevs = bytearray(self._rbcrevslen * _rbcrecsize)

    def branchinfo(self, rev):
        """Return branch name and close flag for rev, using and updating
        persistent cache."""
        changelog = self._repo.changelog
        rbcrevidx = rev * _rbcrecsize

        # avoid negative index, changelog.read(nullrev) is fast without cache
        if rev == nullrev:
            return changelog.branchinfo(rev)

        # if requested rev isn't allocated, grow and cache the rev info
        if len(self._rbcrevs) < rbcrevidx + _rbcrecsize:
            return self._branchinfo(rev)

        # fast path: extract data from cache, use it if node is matching
        reponode = changelog.node(rev)[:_rbcnodelen]
        cachenode, branchidx = unpack_from(
            _rbcrecfmt, util.buffer(self._rbcrevs), rbcrevidx
        )
        close = bool(branchidx & _rbccloseflag)
        if close:
            branchidx &= _rbcbranchidxmask
        if cachenode == "\0\0\0\0":
            pass
        elif cachenode == reponode:
            try:
                return self._names[branchidx], close
            except IndexError:
                # recover from invalid reference to unknown branch
                self._repo.ui.debug(
                    "referenced branch names not found"
                    " - rebuilding revision branch cache from scratch\n"
                )
                self._clear()
        else:
            # rev/node map has changed, invalidate the cache from here up
            self._repo.ui.debug(
                "history modification detected - truncating "
                "revision branch cache to revision %d\n" % rev
            )
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
        if rev == nullrev:
            return
        rbcrevidx = rev * _rbcrecsize
        if len(self._rbcrevs) < rbcrevidx + _rbcrecsize:
            self._rbcrevs.extend(
                "\0" * (len(self._repo.changelog) * _rbcrecsize - len(self._rbcrevs))
            )
        pack_into(_rbcrecfmt, self._rbcrevs, rbcrevidx, node, branchidx)
        self._rbcrevslen = min(self._rbcrevslen, rev)

        tr = self._repo.currenttransaction()
        if tr:
            tr.addfinalize("write-revbranchcache", self.write)

    def write(self, tr=None):
        """Save branch cache if it is dirty."""
        repo = self._repo
        wlock = None
        step = ""
        try:
            if self._rbcnamescount < len(self._names):
                step = " names"
                wlock = repo.wlock(wait=False)
                if self._rbcnamescount != 0:
                    f = repo.cachevfs.open(_rbcnames, "ab")
                    if f.tell() == self._rbcsnameslen:
                        f.write("\0")
                    else:
                        f.close()
                        repo.ui.debug("%s changed - rewriting it\n" % _rbcnames)
                        self._rbcnamescount = 0
                        self._rbcrevslen = 0
                if self._rbcnamescount == 0:
                    # before rewriting names, make sure references are removed
                    repo.cachevfs.unlinkpath(_rbcrevs, ignoremissing=True)
                    f = repo.cachevfs.open(_rbcnames, "wb")
                f.write(
                    "\0".join(
                        encoding.fromlocal(b)
                        for b in self._names[self._rbcnamescount :]
                    )
                )
                self._rbcsnameslen = f.tell()
                f.close()
                self._rbcnamescount = len(self._names)

            start = self._rbcrevslen * _rbcrecsize
            if start != len(self._rbcrevs):
                step = ""
                if wlock is None:
                    wlock = repo.wlock(wait=False)
                revs = min(len(repo.changelog), len(self._rbcrevs) // _rbcrecsize)
                f = repo.cachevfs.open(_rbcrevs, "ab")
                if f.tell() != start:
                    repo.ui.debug("truncating cache/%s to %d\n" % (_rbcrevs, start))
                    f.seek(start)
                    if f.tell() != start:
                        start = 0
                        f.seek(start)
                    f.truncate()
                end = revs * _rbcrecsize
                f.write(self._rbcrevs[start:end])
                f.close()
                self._rbcrevslen = revs
        except (IOError, OSError, error.Abort, error.LockError) as inst:
            repo.ui.debug("couldn't write revision branch cache%s: %s\n" % (step, inst))
        finally:
            if wlock is not None:
                wlock.release()
