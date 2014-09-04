# repoview.py - Filtered view of a localrepo object
#
# Copyright 2012 Pierre-Yves David <pierre-yves.david@ens-lyon.org>
#                Logilab SA        <contact@logilab.fr>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import copy
import error
import phases
import util
import obsolete
import struct
import tags as tagsmod
from mercurial.i18n import _

def hideablerevs(repo):
    """Revisions candidates to be hidden

    This is a standalone function to help extensions to wrap it."""
    return obsolete.getrevs(repo, 'obsolete')

def _getstaticblockers(repo):
    """Cacheable revisions blocking hidden changesets from being filtered.

    Additional non-cached hidden blockers are computed in _getdynamicblockers.
    This is a standalone function to help extensions to wrap it."""
    assert not repo.changelog.filteredrevs
    hideable = hideablerevs(repo)
    blockers = set()
    if hideable:
        # We use cl to avoid recursive lookup from repo[xxx]
        cl = repo.changelog
        firsthideable = min(hideable)
        revs = cl.revs(start=firsthideable)
        tofilter = repo.revs(
            '(%ld) and children(%ld)', list(revs), list(hideable))
        blockers.update([r for r in tofilter if r not in hideable])
    return blockers

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
        try:
            wlock = repo.wlock(wait=False)
            # write cache to file
            newhash = cachehash(repo, hideable)
            sortedset = sorted(hidden)
            data = struct.pack('>%ii' % len(sortedset), *sortedset)
            fh = repo.vfs.open(cachefile, 'w+b', atomictemp=True)
            fh.write(struct.pack(">H", cacheversion))
            fh.write(newhash)
            fh.write(data)
        except (IOError, OSError):
            repo.ui.debug('error writing hidden changesets cache')
        except error.LockHeld:
            repo.ui.debug('cannot obtain lock to write hidden changesets cache')
    finally:
        if fh:
            fh.close()
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
            blocked = cl.ancestors(_getstaticblockers(repo), inclusive=True)
            hidden = frozenset(r for r in hideable if r not in blocked)
            trywritehiddencache(repo, hideable, hidden)
        elif repo.ui.configbool('experimental', 'verifyhiddencache', True):
            blocked = cl.ancestors(_getstaticblockers(repo), inclusive=True)
            computed = frozenset(r for r in hideable if r not in blocked)
            if computed != hidden:
                trywritehiddencache(repo, hideable, computed)
                repo.ui.warn(_('Cache inconsistency detected. Please ' +
                    'open an issue on http://bz.selenic.com.\n'))
                hidden = computed

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
    if util.any(repo._phasecache.phaseroots[1:]):
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
        revs = filterrevs(unfi, self.filtername)
        cl = self._clcache
        newkey = (len(unfichangelog), unfichangelog.tip(), hash(revs))
        if cl is not None:
            # we need to check curkey too for some obscure reason.
            # MQ test show a corruption of the underlying repo (in _clcache)
            # without change in the cachekey.
            oldfilter = cl.filteredrevs
            try:
                cl.filteredrevs = ()  # disable filtering for tip
                curkey = (len(cl), cl.tip(), hash(oldfilter))
            finally:
                cl.filteredrevs = oldfilter
            if newkey != self._clcachekey or newkey != curkey:
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
