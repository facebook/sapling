# repoview.py - Filtered view of a localrepo object
#
# Copyright 2012 Pierre-Yves David <pierre-yves.david@ens-lyon.org>
#                Logilab SA        <contact@logilab.fr>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import copy
import heapq
import struct

from .node import nullrev
from . import (
    error,
    obsolete,
    phases,
    tags as tagsmod,
    util,
)

def hideablerevs(repo):
    """Revision candidates to be hidden

    This is a standalone function to allow extensions to wrap it.

    Because we use the set of immutable changesets as a fallback subset in
    branchmap (see mercurial.branchmap.subsettable), you cannot set "public"
    changesets as "hideable". Doing so would break multiple code assertions and
    lead to crashes."""
    return obsolete.getrevs(repo, 'obsolete')

def _getstatichidden(repo):
    """Revision to be hidden (disregarding dynamic blocker)

    To keep a consistent graph, we cannot hide any revisions with
    non-hidden descendants. This function computes the set of
    revisions that could be hidden while keeping the graph consistent.

    A second pass will be done to apply "dynamic blocker" like bookmarks or
    working directory parents.

    """
    assert not repo.changelog.filteredrevs
    hidden = set(hideablerevs(repo))
    if hidden:
        getphase = repo._phasecache.phase
        getparentrevs = repo.changelog.parentrevs
        # Skip heads which are public (guaranteed to not be hidden)
        heap = [-r for r in repo.changelog.headrevs() if getphase(repo, r)]
        heapq.heapify(heap)
        heappop = heapq.heappop
        heappush = heapq.heappush
        seen = set() # no need to init it with heads, they have no children
        while heap:
            rev = -heappop(heap)
            # All children have been processed so at that point, if no children
            # removed 'rev' from the 'hidden' set, 'rev' is going to be hidden.
            blocker = rev not in hidden
            for parent in getparentrevs(rev):
                if parent == nullrev:
                    continue
                if blocker:
                    # If visible, ensure parent will be visible too
                    hidden.discard(parent)
                # - Avoid adding the same revision twice
                # - Skip nodes which are public (guaranteed to not be hidden)
                pre = len(seen)
                seen.add(parent)
                if pre < len(seen) and getphase(repo, rev):
                    heappush(heap, -parent)
    return hidden

def _getdynamicblockers(repo):
    """Non-cacheable revisions blocking hidden changesets from being filtered.

    Get revisions that will block hidden changesets and are likely to change,
    but unlikely to create hidden blockers. They won't be cached, so be careful
    with adding additional computation."""

    cl = repo.changelog
    blockers = set()
    blockers.update([par.rev() for par in repo[None].parents()])
    blockers.update([cl.rev(bm) for bm in repo._bookmarks.values()])

    tags = {}
    tagsmod.readlocaltags(repo.ui, repo, tags, {})
    if tags:
        rev, nodemap = cl.rev, cl.nodemap
        blockers.update(rev(t[0]) for t in tags.values() if t[0] in nodemap)
    return blockers

cacheversion = 1
cachefile = 'cache/hidden'

def cachehash(repo, hideable):
    """return sha1 hash of repository data to identify a valid cache.

    We calculate a sha1 of repo heads and the content of the obsstore and write
    it to the cache. Upon reading we can easily validate by checking the hash
    against the stored one and discard the cache in case the hashes don't match.
    """
    h = util.sha1()
    h.update(''.join(repo.heads()))
    h.update(str(hash(frozenset(hideable))))
    return h.digest()

def _writehiddencache(cachefile, cachehash, hidden):
    """write hidden data to a cache file"""
    data = struct.pack('>%ii' % len(hidden), *sorted(hidden))
    cachefile.write(struct.pack(">H", cacheversion))
    cachefile.write(cachehash)
    cachefile.write(data)

def trywritehiddencache(repo, hideable, hidden):
    """write cache of hidden changesets to disk

    Will not write the cache if a wlock cannot be obtained lazily.
    The cache consists of a head of 22byte:
       2 byte    version number of the cache
      20 byte    sha1 to validate the cache
     n*4 byte    hidden revs
    """
    wlock = fh = None
    try:
        wlock = repo.wlock(wait=False)
        # write cache to file
        newhash = cachehash(repo, hideable)
        fh = repo.vfs.open(cachefile, 'w+b', atomictemp=True)
        _writehiddencache(fh, newhash, hidden)
        fh.close()
    except (IOError, OSError):
        repo.ui.debug('error writing hidden changesets cache\n')
    except error.LockHeld:
        repo.ui.debug('cannot obtain lock to write hidden changesets cache\n')
    finally:
        if wlock:
            wlock.release()

def tryreadcache(repo, hideable):
    """read a cache if the cache exists and is valid, otherwise returns None."""
    hidden = fh = None
    try:
        if repo.vfs.exists(cachefile):
            fh = repo.vfs.open(cachefile, 'rb')
            version, = struct.unpack(">H", fh.read(2))
            oldhash = fh.read(20)
            newhash = cachehash(repo, hideable)
            if (cacheversion, oldhash) == (version, newhash):
                # cache is valid, so we can start reading the hidden revs
                data = fh.read()
                count = len(data) / 4
                hidden = frozenset(struct.unpack('>%ii' % count, data))
        return hidden
    except struct.error:
        repo.ui.debug('corrupted hidden cache\n')
        # No need to fix the content as it will get rewritten
        return None
    except (IOError, OSError):
        repo.ui.debug('cannot read hidden cache\n')
        return None
    finally:
        if fh:
            fh.close()

def computehidden(repo):
    """compute the set of hidden revision to filter

    During most operation hidden should be filtered."""
    assert not repo.changelog.filteredrevs

    hidden = frozenset()
    hideable = hideablerevs(repo)
    if hideable:
        cl = repo.changelog
        hidden = tryreadcache(repo, hideable)
        if hidden is None:
            hidden = frozenset(_getstatichidden(repo))
            trywritehiddencache(repo, hideable, hidden)

        # check if we have wd parents, bookmarks or tags pointing to hidden
        # changesets and remove those.
        dynamic = hidden & _getdynamicblockers(repo)
        if dynamic:
            blocked = cl.ancestors(dynamic, inclusive=True)
            hidden = frozenset(r for r in hidden if r not in blocked)
    return hidden

def computeunserved(repo):
    """compute the set of revision that should be filtered when used a server

    Secret and hidden changeset should not pretend to be here."""
    assert not repo.changelog.filteredrevs
    # fast path in simple case to avoid impact of non optimised code
    hiddens = filterrevs(repo, 'visible')
    if phases.hassecret(repo):
        cl = repo.changelog
        secret = phases.secret
        getphase = repo._phasecache.phase
        first = min(cl.rev(n) for n in repo._phasecache.phaseroots[secret])
        revs = cl.revs(start=first)
        secrets = set(r for r in revs if getphase(repo, r) >= secret)
        return frozenset(hiddens | secrets)
    else:
        return hiddens

def computemutable(repo):
    """compute the set of revision that should be filtered when used a server

    Secret and hidden changeset should not pretend to be here."""
    assert not repo.changelog.filteredrevs
    # fast check to avoid revset call on huge repo
    if any(repo._phasecache.phaseroots[1:]):
        getphase = repo._phasecache.phase
        maymutable = filterrevs(repo, 'base')
        return frozenset(r for r in maymutable if getphase(repo, r))
    return frozenset()

def computeimpactable(repo):
    """Everything impactable by mutable revision

    The immutable filter still have some chance to get invalidated. This will
    happen when:

    - you garbage collect hidden changeset,
    - public phase is moved backward,
    - something is changed in the filtering (this could be fixed)

    This filter out any mutable changeset and any public changeset that may be
    impacted by something happening to a mutable revision.

    This is achieved by filtered everything with a revision number egal or
    higher than the first mutable changeset is filtered."""
    assert not repo.changelog.filteredrevs
    cl = repo.changelog
    firstmutable = len(cl)
    for roots in repo._phasecache.phaseroots[1:]:
        if roots:
            firstmutable = min(firstmutable, min(cl.rev(r) for r in roots))
    # protect from nullrev root
    firstmutable = max(0, firstmutable)
    return frozenset(xrange(firstmutable, len(cl)))

# function to compute filtered set
#
# When adding a new filter you MUST update the table at:
#     mercurial.branchmap.subsettable
# Otherwise your filter will have to recompute all its branches cache
# from scratch (very slow).
filtertable = {'visible': computehidden,
               'served': computeunserved,
               'immutable':  computemutable,
               'base':  computeimpactable}

def filterrevs(repo, filtername):
    """returns set of filtered revision for this filter name"""
    if filtername not in repo.filteredrevcache:
        func = filtertable[filtername]
        repo.filteredrevcache[filtername] = func(repo.unfiltered())
    return repo.filteredrevcache[filtername]

class repoview(object):
    """Provide a read/write view of a repo through a filtered changelog

    This object is used to access a filtered version of a repository without
    altering the original repository object itself. We can not alter the
    original object for two main reasons:
    - It prevents the use of a repo with multiple filters at the same time. In
      particular when multiple threads are involved.
    - It makes scope of the filtering harder to control.

    This object behaves very closely to the original repository. All attribute
    operations are done on the original repository:
    - An access to `repoview.someattr` actually returns `repo.someattr`,
    - A write to `repoview.someattr` actually sets value of `repo.someattr`,
    - A deletion of `repoview.someattr` actually drops `someattr`
      from `repo.__dict__`.

    The only exception is the `changelog` property. It is overridden to return
    a (surface) copy of `repo.changelog` with some revisions filtered. The
    `filtername` attribute of the view control the revisions that need to be
    filtered.  (the fact the changelog is copied is an implementation detail).

    Unlike attributes, this object intercepts all method calls. This means that
    all methods are run on the `repoview` object with the filtered `changelog`
    property. For this purpose the simple `repoview` class must be mixed with
    the actual class of the repository. This ensures that the resulting
    `repoview` object have the very same methods than the repo object. This
    leads to the property below.

        repoview.method() --> repo.__class__.method(repoview)

    The inheritance has to be done dynamically because `repo` can be of any
    subclasses of `localrepo`. Eg: `bundlerepo` or `statichttprepo`.
    """

    def __init__(self, repo, filtername):
        object.__setattr__(self, '_unfilteredrepo', repo)
        object.__setattr__(self, 'filtername', filtername)
        object.__setattr__(self, '_clcachekey', None)
        object.__setattr__(self, '_clcache', None)

    # not a propertycache on purpose we shall implement a proper cache later
    @property
    def changelog(self):
        """return a filtered version of the changeset

        this changelog must not be used for writing"""
        # some cache may be implemented later
        unfi = self._unfilteredrepo
        unfichangelog = unfi.changelog
        # bypass call to changelog.method
        unfiindex = unfichangelog.index
        unfilen = len(unfiindex) - 1
        unfinode = unfiindex[unfilen - 1][7]

        revs = filterrevs(unfi, self.filtername)
        cl = self._clcache
        newkey = (unfilen, unfinode, hash(revs), unfichangelog._delayed)
        # if cl.index is not unfiindex, unfi.changelog would be
        # recreated, and our clcache refers to garbage object
        if (cl is not None and
            (cl.index is not unfiindex or newkey != self._clcachekey)):
            cl = None
        # could have been made None by the previous if
        if cl is None:
            cl = copy.copy(unfichangelog)
            cl.filteredrevs = revs
            object.__setattr__(self, '_clcache', cl)
            object.__setattr__(self, '_clcachekey', newkey)
        return cl

    def unfiltered(self):
        """Return an unfiltered version of a repo"""
        return self._unfilteredrepo

    def filtered(self, name):
        """Return a filtered version of a repository"""
        if name == self.filtername:
            return self
        return self.unfiltered().filtered(name)

    # everything access are forwarded to the proxied repo
    def __getattr__(self, attr):
        return getattr(self._unfilteredrepo, attr)

    def __setattr__(self, attr, value):
        return setattr(self._unfilteredrepo, attr, value)

    def __delattr__(self, attr):
        return delattr(self._unfilteredrepo, attr)

    # The `requirements` attribute is initialized during __init__. But
    # __getattr__ won't be called as it also exists on the class. We need
    # explicit forwarding to main repo here
    @property
    def requirements(self):
        return self._unfilteredrepo.requirements
