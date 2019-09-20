# repoview.py - Filtered view of a localrepo object
#
# Copyright 2012 Pierre-Yves David <pierre-yves.david@ens-lyon.org>
#                Logilab SA        <contact@logilab.fr>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import copy
import weakref

from . import obsolete, phases, pycompat, tags as tagsmod, visibility
from .node import nullrev


try:
    xrange(0)
except NameError:
    xrange = range


def hideablerevs(repo):
    """Revision candidates to be hidden

    Because we use the set of immutable changesets as a fallback subset in
    branchmap (see mercurial.branchmap.subsettable), you cannot set "public"
    changesets as "hideable". Doing so would break multiple code assertions and
    lead to crashes."""
    if visibility.enabled(repo):
        return visibility.invisiblerevs(repo)
    else:
        return obsolete.getrevs(repo, "obsolete")


def pinnedrevs(repo):
    """revisions blocking hidden changesets from being filtered
    """

    cl = repo.changelog
    pinned = set()
    pinned.update([par.rev() for par in repo[None].parents()])
    pinned.update([cl.rev(bm) for bm in repo._bookmarks.values()])

    tags = {}
    tagsmod.readlocaltags(repo.ui, repo, tags, {})
    if tags:
        rev, nodemap = cl.rev, cl.nodemap
        pinned.update(rev(t[0]) for t in tags.values() if t[0] in nodemap)
    return pinned


def _revealancestors(pfunc, hidden, revs):
    """reveals contiguous chains of hidden ancestors of 'revs' by removing them
    from 'hidden'

    - pfunc(r): a funtion returning parent of 'r',
    - hidden: the (preliminary) hidden revisions, to be updated
    - revs: iterable of revnum,

    (Ancestors are revealed exclusively, i.e. the elements in 'revs' are
    *not* revealed)
    """
    stack = list(revs)
    while stack:
        for p in pfunc(stack.pop()):
            if p != nullrev and p in hidden:
                hidden.remove(p)
                stack.append(p)


def computehidden(repo):
    """compute the set of hidden revision to filter

    During most operation hidden should be filtered."""
    assert not repo.changelog.filteredrevs

    hidden = hideablerevs(repo)
    if hidden:
        hidden = set(hidden - pinnedrevs(repo))
        pfunc = repo.changelog.parentrevs
        mutablephases = (phases.draft, phases.secret)
        mutable = repo._phasecache.getrevset(repo, mutablephases)

        visible = mutable - hidden
        _revealancestors(pfunc, hidden, visible)
    return frozenset(hidden)


def computeunserved(repo):
    """compute the set of revision that should be filtered when used a server

    Secret and hidden changeset should not pretend to be here."""
    assert not repo.changelog.filteredrevs
    # fast path in simple case to avoid impact of non optimised code
    hiddens = filterrevs(repo, "visible")
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
    assert not repo.changelog.filteredrevs
    # fast check to avoid revset call on huge repo
    if any(repo._phasecache.phaseroots[1:]):
        getphase = repo._phasecache.phase
        maymutable = filterrevs(repo, "base")
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
# Otherwise your filter will have to recompute all its branches cache
# from scratch (very slow).
filtertable = {
    "visible": computehidden,
    "served": computeunserved,
    "immutable": computemutable,
    "base": computeimpactable,
}


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
    subclasses of `localrepo`. Eg: `bundlerepo`.
    """

    def __init__(self, repo, filtername):
        object.__setattr__(self, r"_unfilteredrepo", repo)
        object.__setattr__(self, r"filtername", filtername)
        object.__setattr__(self, r"_clcachekey", None)
        object.__setattr__(self, r"_clcache", None)

    # not a propertycache on purpose we shall implement a proper cache later
    @property
    def changelog(self):
        """return a filtered version of the changeset

        this changelog must not be used for writing"""
        # some cache may be implemented later
        unfi = self._unfilteredrepo
        unfichangelog = unfi.changelog
        # if narrow-heads is enabled, no need to filter anything
        if unfi.ui.configbool("experimental", "narrow-heads"):
            return unfichangelog
        # bypass call to changelog.method
        unfiindex = unfichangelog.index
        unfilen = len(unfiindex) - 1
        unfinode = unfiindex[unfilen - 1][7]

        revs = filterrevs(unfi, self.filtername)
        cl = self._clcache
        newkey = (unfilen, unfinode, hash(revs), unfichangelog._delayed)
        # if cl.index is not unfiindex, unfi.changelog would be
        # recreated, and our clcache refers to garbage object
        if cl is not None and (cl.index is not unfiindex or newkey != self._clcachekey):
            cl = None
        # could have been made None by the previous if
        if cl is None:
            cl = copy.copy(unfichangelog)
            cl.filteredrevs = revs
            object.__setattr__(self, r"_clcache", cl)
            object.__setattr__(self, r"_clcachekey", newkey)
        return cl

    def unfiltered(self):
        """Return an unfiltered version of a repo"""
        return self._unfilteredrepo

    def filtered(self, name):
        """Return a filtered version of a repository"""
        if name == self.filtername:
            return self
        return self.unfiltered().filtered(name)

    def __repr__(self):
        return r"<%s:%s %r>" % (
            self.__class__.__name__,
            pycompat.sysstr(self.filtername),
            self.unfiltered(),
        )

    # everything access are forwarded to the proxied repo
    def __getattr__(self, attr):
        return getattr(self._unfilteredrepo, attr)

    def __setattr__(self, attr, value):
        return setattr(self._unfilteredrepo, attr, value)

    def __delattr__(self, attr):
        return delattr(self._unfilteredrepo, attr)


# Python <3.4 easily leaks types via __mro__. See
# https://bugs.python.org/issue17950. We cache dynamically created types
# so they won't be leaked on every invocation of repo.filtered().
_filteredrepotypes = weakref.WeakKeyDictionary()


def newtype(base):
    """Create a new type with the repoview mixin and the given base class"""
    if base not in _filteredrepotypes:

        class filteredrepo(repoview, base):
            pass

        _filteredrepotypes[base] = filteredrepo
    return _filteredrepotypes[base]
