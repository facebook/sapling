# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# smartset.py - data structure for revision set
#
# Copyright 2010 Olivia Mackall <olivia@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import copy
import weakref

import bindings

from . import error, parser, streams, util
from .i18n import _
from .node import nullrev, wdirrev
from .pycompat import range


maxrev = bindings.dag.MAX_ID
dagmod = bindings.dag


def _formatsetrepr(r):
    """Format an optional printable representation of a set

    ========  =================================
    type(r)   example
    ========  =================================
    tuple     ('<not %r>', other)
    str       '<branch closed>'
    callable  lambda: '<branch %r>' % sorted(b)
    object    other
    ========  =================================
    """
    if r is None:
        return ""
    elif isinstance(r, tuple):
        return r[0] % r[1:]
    elif isinstance(r, str):
        return r
    elif callable(r):
        return r()
    else:
        return repr(r)


class abstractsmartset(object):
    def __nonzero__(self):
        """True if the smartset is not empty"""
        raise NotImplementedError()

    __bool__ = __nonzero__

    def repo(self):
        reporef = getattr(self, "_reporef", None)
        if reporef is None:
            raise error.ProgrammingError("%r does not have repo" % self)
        repo = reporef()
        if repo is None:
            raise error.ProgrammingError("repo from %r was released" % self)
        return repo

    def __contains__(self, rev):
        """provide fast membership testing"""
        raise NotImplementedError()

    def __iter__(self):
        """iterate the set using rev numbers (aware of prefetch)"""
        if self.prefetchfields():
            # Go through iterctx for prefetch side-effect
            return (c.rev() for c in self.iterctx())
        else:
            return self.iterrev()

    def iterrev(self):
        """iterate the set using rev numbers (not aware of prefetch)"""
        raise NotImplementedError()

    def iterctx(self):
        """iterate the set using contexes, with prefetch considered"""
        repo = self.repo()
        ctxstream = self._iterctxnoprefetch()
        for field in sorted(self.prefetchfields()):
            if field not in prefetchtable:
                raise error.ProgrammingError(
                    "do not know how to prefetch field %s for ctxstream" % field
                )
            ctxstream = prefetchtable[field](repo, ctxstream)
        return ctxstream

    def _iterctxnoprefetch(self):
        """iterate the set using contexes, without prefetch"""
        repo = self.repo()
        for rev in self.iterrev():
            yield repo[rev]

    # Attributes containing a function to perform a fast iteration in a given
    # direction. A smartset can have none, one, or both defined.
    #
    # Default value is None instead of a function returning None to avoid
    # initializing an iterator just for testing if a fast method exists.
    fastasc = None
    fastdesc = None

    def isascending(self):
        """True if the set will iterate in ascending order"""
        raise NotImplementedError()

    def isdescending(self):
        """True if the set will iterate in descending order"""
        raise NotImplementedError()

    def istopo(self):
        """True if the set will iterate in topographical order"""
        raise NotImplementedError()

    def min(self):
        """return the minimum element in the set"""
        if self.fastasc is None:
            v = min(self)
        else:
            for v in self.fastasc():
                break
            else:
                raise ValueError("arg is an empty sequence")
        self.min = lambda: v
        return v

    def max(self):
        """return the maximum element in the set"""
        if self.fastdesc is None:
            return max(self)
        else:
            for v in self.fastdesc():
                break
            else:
                raise ValueError("arg is an empty sequence")
        self.max = lambda: v
        return v

    def first(self):
        """return the first element in the set (user iteration perspective)

        Return None if the set is empty"""
        raise NotImplementedError()

    def last(self):
        """return the last element in the set (user iteration perspective)

        Return None if the set is empty"""
        raise NotImplementedError()

    def __len__(self):
        """return the length of the smartsets

        This can be expensive on smartset that could be lazy otherwise."""
        raise NotImplementedError()

    def fastlen(self):
        """Returns the length of the set, or None if it cannot be calculated quickly."""
        return None

    def reverse(self):
        """reverse the expected iteration order"""
        raise NotImplementedError()

    def sort(self, reverse=False):
        """get the set to iterate in an ascending or descending order"""
        raise NotImplementedError()

    def __and__(self, other):
        """Returns a new object with the intersection of the two collections.

        This is part of the mandatory API for smartset."""
        if isinstance(other, fullreposet):
            return self
        return self.filter(other.__contains__, condrepr=other, cache=False)

    def __add__(self, other):
        """Returns a new object with the union of the two collections.

        This is part of the mandatory API for smartset."""
        return addset(self, other)

    def __sub__(self, other):
        """Returns a new object with the substraction of the two collections.

        This is part of the mandatory API for smartset."""
        c = other.__contains__
        return self.filter(
            lambda r: not c(r), condrepr=("<not %r>", other), cache=False
        )

    def filter(self, condition, condrepr=None, cache=True):
        """Returns this smartset filtered by condition as a new smartset.

        `condition` is a callable which takes a revision number and returns a
        boolean. Optional `condrepr` provides a printable representation of
        the given `condition`.

        This is part of the mandatory API for smartset."""
        # builtin cannot be cached. but do not needs to
        if cache and util.safehasattr(condition, "func_code"):
            condition = util.cachefunc(condition)
        return filteredset(self, condition, condrepr)

    def slice(self, start, stop):
        """Return new smartset that contains selected elements from this set"""
        if start < 0 or stop < 0:
            raise error.ProgrammingError("negative index not allowed")
        return self._slice(start, stop)

    def _slice(self, start, stop):
        # sub classes may override this. start and stop must not be negative,
        # but start > stop is allowed, which should be an empty set.
        ys = []
        it = iter(self)
        for x in range(start):
            y = next(it, None)
            if y is None:
                break
        for x in range(stop - start):
            y = next(it, None)
            if y is None:
                break
            ys.append(y)
        return baseset(
            ys, datarepr=("slice=%d:%d %r", start, stop, self), repo=self.repo()
        )

    def clone(self):
        return copy.copy(self)

    def prefetch(self, *fields):
        """return a smartset with given fields marked as "need prefetch"

        Available fields:
        - "text": commit message

        Note:
        'iterctx()' respects the prefetch metadata.
        """
        newobj = self.clone()
        newobj._prefetchfields = set(fields) | self.prefetchfields()
        return newobj

    def prefetchbytemplate(self, repo, templ):
        """parse a template string and decide what to prefetch"""
        from . import templater  # avoid cycle

        ast = templater.parseexpandaliases(repo, templ)
        fields = []
        if not ast:
            # empty template, use default
            fields += prefetchtemplatekw.get("", [])
        else:
            for t in parser.walktree(ast):
                if len(t) < 2 or t[0] != "symbol":
                    continue
                fields += prefetchtemplatekw.get(t[1], [])
        return self.prefetch(*fields)

    def prefetchfields(self):
        """get a set of fields to prefetch"""
        return getattr(self, "_prefetchfields", set())


class baseset(abstractsmartset):
    """Basic data structure that represents a revset and contains the basic
    operation that it should be able to perform.

    Every method in this class should be implemented by any smartset class.

    This class could be constructed by an (unordered) set, or an (ordered)
    list-like object. If a set is provided, it'll be sorted lazily.

    >>> x = [4, 0, 7, 6]
    >>> y = [5, 6, 7, 3]
    >>> repo = util.refcell([])

    Construct by a set:
    >>> xs = baseset(set(x), repo=repo)
    >>> ys = baseset(set(y), repo=repo)
    >>> [list(i) for i in [xs + ys, xs & ys, xs - ys]]
    [[0, 4, 6, 7, 3, 5], [6, 7], [0, 4]]
    >>> [type(i).__name__ for i in [xs + ys, xs & ys, xs - ys]]
    ['addset', 'baseset', 'baseset']

    Construct by a list-like:
    >>> xs = baseset(x, repo=repo)
    >>> ys = baseset((i for i in y), repo=repo)
    >>> [list(i) for i in [xs + ys, xs & ys, xs - ys]]
    [[4, 0, 7, 6, 5, 3], [7, 6], [4, 0]]
    >>> [type(i).__name__ for i in [xs + ys, xs & ys, xs - ys]]
    ['addset', 'filteredset', 'filteredset']

    Populate "_set" fields in the lists so set optimization may be used:
    >>> [1 in xs, 3 in ys]
    [False, True]

    Without sort(), results won't be changed:
    >>> [list(i) for i in [xs + ys, xs & ys, xs - ys]]
    [[4, 0, 7, 6, 5, 3], [7, 6], [4, 0]]
    >>> [type(i).__name__ for i in [xs + ys, xs & ys, xs - ys]]
    ['addset', 'filteredset', 'filteredset']

    With sort(), set optimization could be used:
    >>> xs.sort(reverse=True)
    >>> [list(i) for i in [xs + ys, xs & ys, xs - ys]]
    [[7, 6, 4, 0, 5, 3], [7, 6], [4, 0]]
    >>> [type(i).__name__ for i in [xs + ys, xs & ys, xs - ys]]
    ['addset', 'baseset', 'baseset']

    >>> ys.sort()
    >>> [list(i) for i in [xs + ys, xs & ys, xs - ys]]
    [[7, 6, 4, 0, 3, 5], [7, 6], [4, 0]]
    >>> [type(i).__name__ for i in [xs + ys, xs & ys, xs - ys]]
    ['addset', 'baseset', 'baseset']

    istopo is preserved across set operations
    >>> xs = baseset(set(x), istopo=True, repo=repo)
    >>> rs = xs & ys
    >>> type(rs).__name__
    'baseset'
    >>> rs._istopo
    True
    """

    def __init__(self, data=(), datarepr=None, istopo=False, repo=None):
        """
        datarepr: a tuple of (format, obj, ...), a function or an object that
                  provides a printable representation of the given data.
        """
        self._ascending = None
        self._istopo = istopo
        if isinstance(data, set):
            # converting set to list has a cost, do it lazily
            self._set = data
            # set has no order we pick one for stability purpose
            self._ascending = True
        else:
            if not isinstance(data, list):
                data = list(data)
            self._list = data
        self._datarepr = datarepr
        if repo is None:
            raise TypeError("baseset requires repo")
        self._reporef = weakref.ref(repo)

    @util.propertycache
    def _set(self):
        return set(self._list)

    @util.propertycache
    def _asclist(self):
        asclist = self._list[:]
        asclist.sort()
        return asclist

    @util.propertycache
    def _list(self):
        # _list is only lazily constructed if we have _set
        assert r"_set" in self.__dict__
        return list(self._set)

    def iterrev(self):
        if self._ascending is None:
            return iter(self._list)
        elif self._ascending:
            return iter(self._asclist)
        else:
            return reversed(self._asclist)

    def fastasc(self):
        return iter(self._asclist)

    def fastdesc(self):
        return reversed(self._asclist)

    @util.propertycache
    def __contains__(self):
        return self._set.__contains__

    def __nonzero__(self):
        return bool(len(self))

    __bool__ = __nonzero__

    def sort(self, reverse=False):
        self._ascending = not bool(reverse)
        self._istopo = False

    def reverse(self):
        if self._ascending is None:
            self._list.reverse()
        else:
            self._ascending = not self._ascending
        self._istopo = False

    def __len__(self):
        if "_list" in self.__dict__:
            return len(self._list)
        else:
            return len(self._set)

    fastlen = __len__

    def isascending(self):
        """Returns True if the collection is ascending order, False if not.

        This is part of the mandatory API for smartset."""
        if len(self) <= 1:
            return True
        return self._ascending is not None and self._ascending

    def isdescending(self):
        """Returns True if the collection is descending order, False if not.

        This is part of the mandatory API for smartset."""
        if len(self) <= 1:
            return True
        return self._ascending is not None and not self._ascending

    def istopo(self):
        """Is the collection is in topographical order or not.

        This is part of the mandatory API for smartset."""
        if len(self) <= 1:
            return True
        return self._istopo

    def first(self):
        if self:
            if self._ascending is None:
                return self._list[0]
            elif self._ascending:
                return self._asclist[0]
            else:
                return self._asclist[-1]
        return None

    def last(self):
        if self:
            if self._ascending is None:
                return self._list[-1]
            elif self._ascending:
                return self._asclist[-1]
            else:
                return self._asclist[0]
        return None

    def _fastsetop(self, other, op):
        # try to use native set operations as fast paths
        if (
            type(other) is baseset
            and r"_set" in other.__dict__
            and r"_set" in self.__dict__
            and self._ascending is not None
        ):
            s = baseset(
                data=getattr(self._set, op)(other._set),
                istopo=self._istopo,
                repo=self.repo(),
            )
            s._ascending = self._ascending
        elif type(other) is nameset:
            # Convert to nameset first, then use nameset fastpath
            s = getattr(self._tonameset(), op)(other)
        else:
            s = getattr(super(baseset, self), op)(other)
        return s

    def _tonameset(self):
        cl = self.repo().changelog
        nodes = cl.tonodes(self._list)
        s = cl.torevset(nodes, reverse=not self.isdescending())
        return s

    def __and__(self, other):
        return self._fastsetop(other, "__and__")

    def __sub__(self, other):
        return self._fastsetop(other, "__sub__")

    def _slice(self, start, stop):
        # creating new list should be generally cheaper than iterating items
        if self._ascending is None:
            return baseset(
                self._list[start:stop], istopo=self._istopo, repo=self.repo()
            )

        data = self._asclist
        if not self._ascending:
            start, stop = max(len(data) - stop, 0), max(len(data) - start, 0)
        s = baseset(data[start:stop], istopo=self._istopo, repo=self.repo())
        s._ascending = self._ascending
        return s

    def __repr__(self):
        d = {None: "", False: "-", True: "+"}[self._ascending]
        s = _formatsetrepr(self._datarepr)
        if not s:
            l = self._list
            # if _list has been built from a set, it might have a different
            # order from one python implementation to another.
            # We fallback to the sorted version for a stable output.
            if self._ascending is not None:
                l = self._asclist
            s = repr(l)
        return "<%s%s %s>" % (type(self).__name__, d, s)


class idset(abstractsmartset):
    """Wrapper around Rust's IdSet that meets the smartset interface.

    The Rust SpanSet does not keep order. This structure keeps orders.

    >>> repo = util.refcell([])
    >>> xs = idset([1, 3, 2, 4, 11, 10], repo=repo)
    >>> ys = idset([2, 3, 4, 5, 20], repo=repo)

    >>> xs
    <idset- [1..=4 10 11]>
    >>> ys
    <idset- [2..=5 20]>

    Iteration

    >>> list(xs)
    [11, 10, 4, 3, 2, 1]

    >>> xs.reverse()
    >>> list(xs)
    [1, 2, 3, 4, 10, 11]

    >>> ys.sort()
    >>> list(ys)
    [2, 3, 4, 5, 20]
    >>> ys.first()
    2
    >>> ys.last()
    20

    >>> ys.sort(reverse=True)
    >>> list(ys)
    [20, 5, 4, 3, 2]
    >>> ys.first()
    20
    >>> ys.last()
    2

    Length, contains, min, max

    >>> len(xs)
    6
    >>> 1 in xs
    True
    >>> 5 in xs
    False
    >>> xs.min()
    1
    >>> xs.max()
    11

    Set operations

    >>> xs & ys
    <idset+ [2 3 4]>
    >>> xs - ys
    <idset+ [1 10 11]>
    >>> xs + ys
    <idset+ [1..=5 10 11 20]>
    """

    def __init__(self, spans, repo):
        """data: a dag.spans object, or an iterable of revs"""
        self._spans = dagmod.spans(spans)
        self._ascending = False
        self._reporef = weakref.ref(repo)

    @staticmethod
    def range(repo, start, end, ascending=False):
        """start and end are inclusive, repo is used to filter out invalid revs

        If start > end, an empty set will be returned.
        """
        if start > end:
            spans = dagmod.spans([])
        else:
            spans = dagmod.spans.unsaferange(start, end)
            # Filter by the fullreposet to remove invalid revs.
            cl = repo.changelog
            dag = cl.dag
            allspans = cl.torevs(dag.all())
            spans = spans & allspans
        # Convert from Rust to Python object.
        s = idset(spans, repo=repo)
        s._ascending = ascending
        return s

    def iterrev(self):
        if self._ascending:
            return self.fastasc()
        else:
            return self.fastdesc()

    def _reversediter(self):
        if self._ascending:
            return self.fastasc()
        else:
            return self.fastdesc()

    def fastasc(self):
        return self._spans.iterasc()

    def fastdesc(self):
        return self._spans.iterdesc()

    @util.propertycache
    def __contains__(self):
        return self._spans.__contains__

    def __nonzero__(self):
        return bool(len(self))

    __bool__ = __nonzero__

    def sort(self, reverse=False):
        self._ascending = not bool(reverse)

    def reverse(self):
        self._ascending = not self._ascending

    def __len__(self):
        return len(self._spans)

    fastlen = __len__

    def isascending(self):
        """Returns True if the collection is ascending order, False if not.

        This is part of the mandatory API for smartset."""
        if len(self) <= 1:
            return True
        return self._ascending

    def isdescending(self):
        """Returns True if the collection is descending order, False if not.

        This is part of the mandatory API for smartset."""
        if len(self) <= 1:
            return True
        return not self._ascending

    def istopo(self):
        """Is the collection is in topographical order or not.

        This is part of the mandatory API for smartset."""
        if len(self) <= 1:
            return True
        return False

    def min(self):
        return self._spans.min()

    def max(self):
        return self._spans.max()

    def first(self):
        if self._ascending:
            return self.min()
        else:
            return self.max()

    def last(self):
        if self._ascending:
            return self.max()
        else:
            return self.min()

    def _fastsetop(self, other, op):
        # try to use native set operations as fast paths
        if type(other) is idset:
            s = idset(getattr(self._spans, op)(other._spans), repo=self.repo())
            s._ascending = self._ascending
        elif type(other) is baseset and (
            op != "__add__" or all(r not in other for r in (nullrev, wdirrev))
        ):
            # baseset is cheap to convert. convert it on the fly, but do not
            # convert if it has troublesome virtual revs and the operation is
            # "__add__" (union).
            s = idset(getattr(self._spans, op)(dagmod.spans(other)), repo=self.repo())
            s._ascending = self._ascending
        else:
            # slow path
            s = getattr(super(idset, self), op)(other)
        return s

    def __and__(self, other):
        return self._fastsetop(other, "__and__")

    def __sub__(self, other):
        return self._fastsetop(other, "__sub__")

    def __add__(self, other):
        # XXX: This is an aggressive optimization. It does not respect orders
        # if 'other' is also a idset.
        return self._fastsetop(other, "__add__")

    def __repr__(self):
        d = {False: "-", True: "+"}[self._ascending]
        return "<%s%s %s>" % (type(self).__name__, d, self._spans)


class nameset(abstractsmartset):
    """Wrapper around Rust's NameSet that meets the smartset interface.

    The Rust NameSet uses commit hashes for its public interface.
    This object does conversions to fit in the abstractsmartset interface
    which uses revision numbers.

    Unlike idset, this object can preserve more types of Rust set,
    including lazy sets.
    """

    def __init__(self, changelog, nameset, reverse=False, repo=None):
        assert isinstance(nameset, bindings.dag.nameset)
        self._changelog = changelog
        self._set = nameset
        # This controls the order of the set.
        self._reversed = reverse
        if repo is None:
            raise TypeError("nameset requires repo")
        self._reporef = weakref.ref(repo)

    @property
    def _torev(self):
        return self._changelog.idmap.node2id

    @property
    def _tonode(self):
        return self._changelog.idmap.id2node

    @staticmethod
    def range(repo, start, end, ascending=False):
        """start and end are inclusive, repo is used to filter out invalid revs

        If start > end, an empty set will be returned.
        """
        cl = repo.changelog
        dag = cl.dag
        if start > end:
            spans = dagmod.spans([])
        else:
            spans = dagmod.spans.unsaferange(start, end)
            # Filter by the fullreposet to remove invalid revs.
            allspans = cl.torevs(dag.all())
            spans = spans & allspans
        s = nameset(cl, cl.tonodes(spans), reverse=ascending, repo=repo)
        return s

    @property
    def fastasc(self):
        hints = self._set.hints()
        if hints.get("asc") and not self.prefetchfields():

            def getiter(it=self._set.iter(), torev=self._torev):
                for node in it:
                    yield torev(node)

            return getiter
        return None

    @property
    def fastdesc(self):
        hints = self._set.hints()
        if hints.get("desc") and not self.prefetchfields():

            def getiter(it=self._set.iter(), torev=self._torev):
                for node in it:
                    yield torev(node)

            return getiter
        return None

    def iterrev(self):
        it = self._iternode()
        torev = self._torev
        for node in it:
            yield torev(node)

    def _iternode(self):
        """iterate the set using nodes"""
        if self._reversed:
            return self._set.iterrev()
        else:
            return self._set.iter()

    def _iterctxnoprefetch(self):
        repo = self.repo()
        for n in self._iternode():
            yield repo[n]

    def __contains__(self, rev):
        if rev == nullrev:
            # NameSet does not contain virtual "null" ids.
            # Do not bother looking up remotely.
            return False
        try:
            node = self._tonode(rev)
        except error.CommitLookupError:
            return False
        return node in self._set

    def __nonzero__(self):
        return bool(self._set.first())

    __bool__ = __nonzero__

    @property
    def _ascending(self):
        hints = self._set.hints()
        if hints.get("desc"):
            result = False
        elif hints.get("asc"):
            result = True
        else:
            result = None
        if self._reversed and result is not None:
            result = not result
        return result

    def sort(self, reverse=False):
        if self._ascending is None:
            self._set = self._changelog.dag.sort(self._set)
        self._reversed = False
        if reverse:
            # want desc
            self._reversed = self._ascending is True
        else:
            # want asc
            self._reversed = self._ascending is False

    def reverse(self):
        self._reversed = not self._reversed

    def __len__(self):
        return len(self._set)

    fastlen = __len__

    def isascending(self):
        if self._reversed:
            return bool(self._set.hints().get("desc"))
        else:
            return bool(self._set.hints().get("asc"))

    def isdescending(self):
        if self._reversed:
            return bool(self._set.hints().get("asc"))
        else:
            return bool(self._set.hints().get("desc"))

    def istopo(self):
        return False

    def first(self):
        if self._reversed:
            node = self._set.last()
        else:
            node = self._set.first()
        if node:
            return self._torev(node)

    def last(self):
        if self._reversed:
            node = self._set.first()
        else:
            node = self._set.last()
        if node:
            return self._torev(node)

    def min(self):
        hints = self._set.hints()
        if hints.get("desc"):
            result = self._set.last()
        elif hints.get("asc"):
            result = self._set.first()
        else:
            result = None
        if result is None:
            result = min(self)
        else:
            result = self._torev(result)
        self.min = lambda: result
        return result

    def max(self):
        hints = self._set.hints()
        if hints.get("desc"):
            result = self._set.first()
        elif hints.get("asc"):
            result = self._set.last()
        else:
            result = None
        if result is None:
            result = max(self)
        else:
            result = self._torev(result)
        self.max = lambda: result
        return result

    def _setop(self, other, op):
        # try to use native set operations as fast paths

        # Extract the Rust binding object.
        ty = type(other)
        if ty is idset:
            # convert idset to nameset
            otherset = self._changelog.tonodes(other)
        elif ty is nameset:
            otherset = other._set
        elif ty is baseset:
            # convert basesee to nameset
            otherset = self._changelog.tonodes(other._list)
        else:
            otherset = None
        if otherset is not None:
            # set operation by the Rust layer
            newset = getattr(self._set, op)(otherset)
            s = nameset(self._changelog, newset, repo=self.repo())
            # preserve order
            if self.isascending():
                s.sort()
            elif self.isdescending():
                s.sort(reverse=True)
        else:
            # slow path
            s = getattr(super(nameset, self), op)(other)
        return s

    def __and__(self, other):
        return self._setop(other, "__and__")

    def __sub__(self, other):
        return self._setop(other, "__sub__")

    def __add__(self, other):
        # XXX: This is an aggressive optimization. It does not always respect
        # orders.
        return self._setop(other, "__add__")

    def _slice(self, start, stop):
        # sub classes may override this. start and stop must not be negative,
        # but start > stop is allowed, which should be an empty set.
        take = stop - start
        skip = start

        if self._reversed:
            # The Rust set does not support order.
            #
            # Translate
            #   [<---------------]
            #         [take][skip]
            # to:
            #   [--------------->]
            #   [skip][take]
            skip = len(self) - take - start
            if skip < 0:
                # Translate
                #   [-skip][-------------->]
                #   [   take    ]
                # to:
                #          [-------------->]
                #          [take]
                take -= -skip
                skip = 0

        repo = self.repo()
        if take <= 0:
            return baseset([], repo=repo)

        newset = self._set.skip(skip).take(take)
        s = nameset(self._changelog, newset, repo=self.repo())
        # preserve order
        if self.isascending():
            s.sort()
        elif self.isdescending():
            s.sort(reverse=True)
        return s

    def __repr__(self):
        d = {False: "-", True: "+", None: ""}[self._ascending]
        return "<%s%s %s>" % (type(self).__name__, d, self._set)


class filteredset(abstractsmartset):
    """Duck type for baseset class which iterates lazily over the revisions in
    the subset and contains a function which tests for membership in the
    revset
    """

    def __init__(self, subset, condition=lambda x: True, condrepr=None):
        """
        condition: a function that decide whether a revision in the subset
                   belongs to the revset or not.
        condrepr: a tuple of (format, obj, ...), a function or an object that
                  provides a printable representation of the given condition.
        """
        self._subset = subset
        self._condition = condition
        self._condrepr = condrepr
        self._reporef = getattr(subset, "_reporef", None)

    def __contains__(self, x):
        return x in self._subset and self._condition(x)

    def iterrev(self):
        return self._iterfilter(self._subset)

    def _progressmodel(self):
        """Return the Rust ProgressBar model.

        Changing the model and a Rust thread (if configured) will render the
        progress bar.
        """
        bar = bindings.progress.model.ProgressBar(
            _("filtering"), self._subset.fastlen(), _("commits")
        )
        fields = self._subset.prefetchfields()
        if fields:
            bar.set_message(_("(prefetch %s)") % ", ".join(sorted(fields)))
        return bar

    def _iterfilter(self, it):
        cond = self._condition
        bar = self._progressmodel()
        inc = bar.increase_position
        for x in it:
            inc(1)
            if cond(x):
                yield x

    def _iterctxnoprefetch(self):
        # respect subset's prefetch settings
        ctxstream = self._subset.iterctx()
        cond = self._condition
        bar = self._progressmodel()
        inc = bar.increase_position
        for ctx in ctxstream:
            inc(1)
            if cond(ctx.rev()):
                yield ctx

    @property
    def fastasc(self):
        it = self._subset.fastasc
        if it is None:
            return None
        return lambda: self._iterfilter(it())

    @property
    def fastdesc(self):
        it = self._subset.fastdesc
        if it is None:
            return None
        return lambda: self._iterfilter(it())

    def __nonzero__(self):
        fast = None
        candidates = [
            self.fastasc if self.isascending() else None,
            self.fastdesc if self.isdescending() else None,
            self.fastasc,
            self.fastdesc,
        ]
        for candidate in candidates:
            if candidate is not None:
                fast = candidate
                break

        if fast is not None:
            it = fast()
        else:
            it = self

        for r in it:
            return True
        return False

    __bool__ = __nonzero__

    def __len__(self):
        # Basic implementation to be changed in future patches.
        # until this gets improved, we use generator expression
        # here, since list comprehensions are free to call __len__ again
        # causing infinite recursion
        count = 0
        for r in self:
            count += 1
        return count

    def sort(self, reverse=False):
        self._subset.sort(reverse=reverse)

    def reverse(self):
        self._subset.reverse()

    def isascending(self):
        return self._subset.isascending()

    def isdescending(self):
        return self._subset.isdescending()

    def istopo(self):
        return self._subset.istopo()

    def first(self):
        for x in self:
            return x
        return None

    def last(self):
        it = None
        if self.isascending():
            it = self.fastdesc
        elif self.isdescending():
            it = self.fastasc
        if it is not None:
            for x in it():
                return x
            return None  # empty case
        else:
            x = None
            for x in self:
                pass
            return x

    def __repr__(self):
        xs = [repr(self._subset)]
        s = _formatsetrepr(self._condrepr)
        if s:
            xs.append(s)
        return "<%s %s>" % (type(self).__name__, ", ".join(xs))


def _iterordered(ascending, iter1, iter2):
    """produce an ordered iteration from two iterators with the same order

    The ascending is used to indicated the iteration direction.
    """
    choice = max
    if ascending:
        choice = min

    val1 = None
    val2 = None
    try:
        # Consume both iterators in an ordered way until one is empty
        while True:
            if val1 is None:
                val1 = next(iter1)
            if val2 is None:
                val2 = next(iter2)
            n = choice(val1, val2)
            yield n
            if val1 == n:
                val1 = None
            if val2 == n:
                val2 = None
    except StopIteration:
        # Flush any remaining values and consume the other one
        it = iter2
        if val1 is not None:
            yield val1
            it = iter1
        elif val2 is not None:
            # might have been equality and both are empty
            yield val2
        for val in it:
            yield val


class addset(abstractsmartset):
    """Represent the addition of two sets

    Wrapper structure for lazily adding two structures without losing much
    performance on the __contains__ method

    If the ascending attribute is set, that means the two structures are
    ordered in either an ascending or descending way. Therefore, we can add
    them maintaining the order by iterating over both at the same time

    >>> repo = util.refcell([])
    >>> xs = baseset([0, 3, 2], repo=repo)
    >>> ys = baseset([5, 2, 4], repo=repo)

    >>> rs = addset(xs, ys)
    >>> bool(rs), 0 in rs, 1 in rs, 5 in rs, rs.first(), rs.last()
    (True, True, False, True, 0, 4)
    >>> rs = addset(xs, baseset([], repo=repo))
    >>> bool(rs), 0 in rs, 1 in rs, rs.first(), rs.last()
    (True, True, False, 0, 2)
    >>> rs = addset(baseset([], repo=repo), baseset([], repo=repo))
    >>> bool(rs), 0 in rs, rs.first(), rs.last()
    (False, False, None, None)

    iterate unsorted:
    >>> rs = addset(xs, ys)
    >>> # (use generator because pypy could call len())
    >>> list(x for x in rs)  # without _genlist
    [0, 3, 2, 5, 4]
    >>> assert not rs._genlist
    >>> len(rs)
    5
    >>> [x for x in rs]  # with _genlist
    [0, 3, 2, 5, 4]
    >>> assert rs._genlist

    iterate ascending:
    >>> rs = addset(xs, ys, ascending=True)
    >>> # (use generator because pypy could call len())
    >>> list(x for x in rs), list(x for x in rs.fastasc())  # without _asclist
    ([0, 2, 3, 4, 5], [0, 2, 3, 4, 5])
    >>> assert not rs._asclist
    >>> len(rs)
    5
    >>> [x for x in rs], [x for x in rs.fastasc()]
    ([0, 2, 3, 4, 5], [0, 2, 3, 4, 5])
    >>> assert rs._asclist

    iterate descending:
    >>> rs = addset(xs, ys, ascending=False)
    >>> # (use generator because pypy could call len())
    >>> list(x for x in rs), list(x for x in rs.fastdesc())  # without _asclist
    ([5, 4, 3, 2, 0], [5, 4, 3, 2, 0])
    >>> assert not rs._asclist
    >>> len(rs)
    5
    >>> [x for x in rs], [x for x in rs.fastdesc()]
    ([5, 4, 3, 2, 0], [5, 4, 3, 2, 0])
    >>> assert rs._asclist

    iterate ascending without fastasc:
    >>> rs = addset(xs, generatorset(ys, repo=repo), ascending=True)
    >>> assert rs.fastasc is None
    >>> [x for x in rs]
    [0, 2, 3, 4, 5]

    iterate descending without fastdesc:
    >>> rs = addset(generatorset(xs, repo=repo), ys, ascending=False)
    >>> assert rs.fastdesc is None
    >>> [x for x in rs]
    [5, 4, 3, 2, 0]
    """

    def __init__(self, revs1, revs2, ascending=None):
        self._r1 = revs1
        self._r2 = revs2
        self._iter = None
        self._ascending = ascending
        self._genlist = None
        self._asclist = None
        self._reporef = getattr(revs1, "_reporef", getattr(revs2, "_reporef", None))

    def __len__(self):
        return len(self._list)

    def __nonzero__(self):
        return bool(self._r1) or bool(self._r2)

    __bool__ = __nonzero__

    @util.propertycache
    def _list(self):
        if not self._genlist:
            self._genlist = baseset(iter(self), repo=self.repo())
        return self._genlist

    def iterrev(self):
        """Iterate over both collections without repeating elements

        If the ascending attribute is not set, iterate over the first one and
        then over the second one checking for membership on the first one so we
        dont yield any duplicates.

        If the ascending attribute is set, iterate over both collections at the
        same time, yielding only one value at a time in the given order.
        """
        if self._ascending is None:
            if self._genlist:
                return iter(self._genlist)

            def arbitraryordergen():
                for r in self._r1:
                    yield r
                inr1 = self._r1.__contains__
                for r in self._r2:
                    if not inr1(r):
                        yield r

            return arbitraryordergen()
        # try to use our own fast iterator if it exists
        self._trysetasclist()
        if self._ascending:
            attr = "fastasc"
        else:
            attr = "fastdesc"
        it = getattr(self, attr)
        if it is not None:
            return it()
        # maybe half of the component supports fast
        # get iterator for _r1
        iter1 = getattr(self._r1, attr)
        if iter1 is None:
            # let's avoid side effect (not sure it matters)
            iter1 = iter(sorted(self._r1, reverse=not self._ascending))
        else:
            iter1 = iter1()
        # get iterator for _r2
        iter2 = getattr(self._r2, attr)
        if iter2 is None:
            # let's avoid side effect (not sure it matters)
            iter2 = iter(sorted(self._r2, reverse=not self._ascending))
        else:
            iter2 = iter2()
        return _iterordered(self._ascending, iter1, iter2)

    def _trysetasclist(self):
        """populate the _asclist attribute if possible and necessary"""
        if self._genlist is not None and self._asclist is None:
            self._asclist = sorted(self._genlist)

    @property
    def fastasc(self):
        self._trysetasclist()
        if self._asclist is not None:
            return self._asclist.__iter__
        iter1 = self._r1.fastasc
        iter2 = self._r2.fastasc
        if None in (iter1, iter2):
            return None
        return lambda: _iterordered(True, iter1(), iter2())

    @property
    def fastdesc(self):
        self._trysetasclist()
        if self._asclist is not None:
            return self._asclist.__reversed__
        iter1 = self._r1.fastdesc
        iter2 = self._r2.fastdesc
        if None in (iter1, iter2):
            return None
        return lambda: _iterordered(False, iter1(), iter2())

    def __contains__(self, x):
        return x in self._r1 or x in self._r2

    def sort(self, reverse=False):
        """Sort the added set

        For this we use the cached list with all the generated values and if we
        know they are ascending or descending we can sort them in a smart way.
        """
        self._ascending = not reverse

    def isascending(self):
        return self._ascending is not None and self._ascending

    def isdescending(self):
        return self._ascending is not None and not self._ascending

    def istopo(self):
        # not worth the trouble asserting if the two sets combined are still
        # in topographical order. Use the sort() predicate to explicitly sort
        # again instead.
        return False

    def reverse(self):
        if self._ascending is None:
            self._list.reverse()
        else:
            self._ascending = not self._ascending

    def first(self):
        for x in self:
            return x
        return None

    def last(self):
        self.reverse()
        val = self.first()
        self.reverse()
        return val

    def __repr__(self):
        d = {None: "", False: "-", True: "+"}[self._ascending]
        return "<%s%s %r, %r>" % (type(self).__name__, d, self._r1, self._r2)


class generatorset(abstractsmartset):
    """Wrap a generator for lazy iteration

    Wrapper structure for generators that provides lazy membership and can
    be iterated more than once.
    When asked for membership it generates values until either it finds the
    requested one or has gone through all the elements in the generator

    >>> repo = util.refcell([])
    >>> xs = generatorset([0, 1, 4], iterasc=True, repo=repo)
    >>> assert xs.last() == xs.last()
    >>> xs.last()  # cached
    4
    """

    def __init__(self, gen, iterasc=None, repo=None):
        """
        gen: a generator producing the values for the generatorset.
        """
        rgen = bindings.threading.RGenerator(gen)
        self._rgen = rgen
        self._containschecked = 0
        self._asclist = None
        self._cache = {}
        self._ascending = True
        if iterasc is not None:
            if iterasc:
                self.fastasc = self._iterator
                self.__contains__ = self._asccontains
            else:
                self.fastdesc = self._iterator
                self.__contains__ = self._desccontains
        self._iterasc = iterasc
        if repo is None:
            raise TypeError("generatorset requires repo")
        self._reporef = weakref.ref(repo)

    @property
    def _finished(self):
        return self._rgen.completed()

    def __nonzero__(self):
        # Do not use 'for r in self' because it will enforce the iteration
        # order (default ascending), possibly unrolling a whole descending
        # iterator.
        if self._rgen.list():
            return True
        try:
            next(self._rgen.iter())
            return True
        except StopIteration:
            return False

    __bool__ = __nonzero__

    def __contains__(self, x):
        cache = self._cache
        if x in cache:
            return cache[x]

        checked = self._containschecked
        for l in self._rgen.iter(skip=checked):
            checked += 1
            cache[l] = True
            if l == x:
                self._containschecked = checked
                return True
        self._containschecked = checked

        cache[x] = False
        return False

    def _asccontains(self, x):
        """version of contains optimised for ascending generator"""
        cache = self._cache
        if x in cache:
            return cache[x]

        checked = self._containschecked
        for l in self._rgen.iter(skip=checked):
            checked += 1
            cache[l] = True
            if l == x:
                self._containschecked = checked
                return True
            if l > x:
                break
        self._containschecked = checked

        cache[x] = False
        return False

    def _desccontains(self, x):
        """version of contains optimised for descending generator"""
        cache = self._cache
        if x in cache:
            return cache[x]

        checked = self._containschecked
        for l in self._rgen.iter(skip=checked):
            checked += 1
            cache[l] = True
            if l == x:
                self._containschecked = checked
                return True
            if l < x:
                break
        self._containschecked = checked

        self._cache[x] = False
        return False

    def iterrev(self):
        if self._ascending:
            it = self.fastasc
        else:
            it = self.fastdesc
        if it is not None:
            return it()
        # we need to consume the iterator
        self._fulllist()
        # recall the same code
        return self.iterrev()

    def _iterator(self):
        if self._finished:
            return iter(self._rgen.list())
        return self._rgen.iter()

    def _fulllist(self):
        if not self._finished:
            self._rgen.itertoend()
            assert self._finished
        if self._asclist is None:
            asc = sorted(self._rgen.list())
            self._asclist = asc
            self.fastasc = asc.__iter__
            self.fastdesc = asc.__reversed__
        return self._rgen.list()

    def __len__(self):
        return len(self._fulllist())

    def sort(self, reverse=False):
        self._ascending = not reverse

    def reverse(self):
        self._ascending = not self._ascending

    def isascending(self):
        return self._ascending

    def isdescending(self):
        return not self._ascending

    def istopo(self):
        # not worth the trouble asserting if the two sets combined are still
        # in topographical order. Use the sort() predicate to explicitly sort
        # again instead.
        return False

    def first(self):
        if self._ascending:
            it = self.fastasc
        else:
            it = self.fastdesc
        if it is None:
            # we need to consume all and try again
            self._fulllist()
            return self.first()
        return next(it(), None)

    def last(self):
        if self._ascending:
            it = self.fastdesc
        else:
            it = self.fastasc
        if it is None:
            # we need to consume all and try again
            self._fulllist()
            return self.last()
        return next(it(), None)

    def __repr__(self):
        d = {False: "-", True: "+"}[self._ascending]
        return "<%s%s>" % (type(self).__name__, d)


def spanset(repo, start=0, end=maxrev):
    """Create a spanset that represents a range of repository revisions

    start: first revision included the set (default to 0)
    end:   first revision excluded (last+1) (default to len(repo))

    Spanset will be descending if `end` < `start`.
    """
    if end is None:
        end = len(repo)
    ascending = start <= end
    if not ascending:
        start, end = min(end, maxrev - 1) + 1, min(start, maxrev - 1) + 1
    s = nameset.range(repo, start, end - 1, ascending)
    # special handling of nullrev
    if start == nullrev:
        if ascending:
            s = baseset([nullrev], repo=repo) + s
        else:
            s = s + baseset([nullrev], repo=repo)
    return s


class fullreposet(idset):
    """a set containing all revisions in the repo

    This class exists to host special optimization and magic to handle virtual
    revisions such as "null".
    """

    def __new__(cls, repo):
        s = idset.range(repo, 0, maxrev, True)
        s.__class__ = cls
        return s

    def clone(self):
        # cannot use copy.copy because __new__ is incompatible
        return fullreposet(self.repo())

    def __init__(cls, repo):
        # __new__ takes care of things
        pass

    def __and__(self, other):
        """As self contains the whole repo, all of the other set should also be
        in self. Therefore `self & other = other`.

        This boldly assumes the other contains valid revs only.
        """
        # other not a smartset, make is so
        if not util.safehasattr(other, "isascending"):
            # filter out hidden revision
            # (this boldly assumes all smartset are pure)
            #
            # `other` was used with "&", let's assume this is a set like
            # object.
            other = baseset(other, repo=self.repo())

        other.sort(reverse=self.isdescending())
        return other


def prettyformat(revs):
    lines = []
    rs = repr(revs)
    p = 0
    while p < len(rs):
        q = rs.find("<", p + 1)
        if q < 0:
            q = len(rs)
        l = rs.count("<", 0, p) - rs.count(">", 0, p)
        assert l >= 0
        lines.append((l, rs[p:q].rstrip()))
        p = q
    return "\n".join("  " * l + s for l, s in lines)


# Given a "prefetch field name" like "text", how to do the prefetch.
# {prefetch_field_name: func(repo, iter[ctx]) -> iter[ctx]}
prefetchtable = {"text": streams.prefetchtextstream}

# Given a template keyword, what "prefetch field name"s are needed.
# {template_symbol: [prefetch_field_name]}
prefetchtemplatekw = {
    "author": ["text"],
    "date": ["text"],
    "desc": ["text"],
    "extras": ["text"],
    "file_adds": ["text"],
    "file_copies_switch": ["text"],
    "file_copies": ["text"],
    "file_dels": ["text"],
    "file_mods": ["text"],
    "filestat": ["text"],
    "files": ["text"],
    "manifest": ["text"],
    # used by default template
    "": ["text"],
}
