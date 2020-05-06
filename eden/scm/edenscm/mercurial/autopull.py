# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# autopull.py - utilities to pull commits automatically.

from __future__ import absolute_import

import re

from . import bookmarks, error, pycompat, registrar, util
from .i18n import _
from .node import hex


_commithashre = re.compile(r"\A[0-9a-f]{6,40}\Z")
_table = {}  # {name: (repo, name) -> Optional[pullattempt]}

builtinautopullpredicate = registrar.autopullpredicate(_table)


class pullattempt(object):
    """Describe an auto-pull attempt"""

    def __init__(
        self, friendlyname=None, headnodes=None, headnames=None, bookmarknames=None
    ):
        # Name to display during pull. ex. "commit abcdef", "bookmark foo".
        self.friendlyname = friendlyname
        # Parameters passed to repo.pull.
        self.headnodes = headnodes or []
        self.headnames = headnames or []
        self.bookmarknames = bookmarknames or []

    def execute(self, repo):
        """Execute the pull on a repo."""
        # TODO: Change source to "default" once Mononoke rolls out to
        # everywhere.
        if self.bookmarknames and "infinitepushbookmark" in repo.ui.paths:
            source = "infinitepushbookmark"
        elif "infinitepush" in repo.ui.paths:
            source = "infinitepush"
        elif "default" in repo.ui.paths:
            source = "default"
        else:
            return

        # Print the "pulling ..." message.
        if self.friendlyname is None:
            name = ", ".join(
                repr(s)
                for s in sorted(
                    set(
                        self.bookmarknames
                        + self.headnames
                        + [hex(n) for n in self.headnodes]
                    )
                )
            )
        else:
            name = self.friendlyname
        url = str(repo.ui.paths.get(source).url)
        repo.ui.status_err(_("pulling %s from %r\n") % (name, url))

        # Pull.
        #
        # When pulling a single commit. Also pull selectivepull bookmarks so
        # it does not end up with lagged master issues.
        bookmarknames = list(self.bookmarknames)
        if self.headnodes or self.headnames:
            bookmarknames += bookmarks.selectivepullbookmarknames(repo, source)
        try:
            repo.pull(
                source,
                bookmarknames=bookmarknames,
                headnodes=self.headnodes,
                headnames=self.headnames,
            )
        except Exception as ex:
            repo.ui.status_err(_("pull failed: %s\n") % ex)

    def merge(self, other):
        """Merge this pullattempt with another pullattempt"""
        self.bookmarknames += other.bookmarknames
        self.headnodes += other.headnodes
        self.headnames += other.headnames
        self.friendlyname = None


def _cachedstringmatcher(pattern, _cache={}):
    # _cache is shared across function calls
    result = _cache.get(pattern)
    if result is None:
        result = util.stringmatcher(pattern)[-1]
        _cache[pattern] = result
    return result


def trypull(repo, xs):
    """Pull the list of given names xs.

    Return true if pull succeeded for all names. Does not raise.
    """
    # Do not attempt to pull the same name twice.
    repo._autopulled = getattr(repo, "_autopulled", set())
    xs = [x for x in xs if x not in repo._autopulled]
    if not xs:
        return False
    repo._autopulled.update(xs)

    # If paths.default is not set. Do not attempt to pull.
    if repo.ui.paths.get("default") is None:
        return False

    def sortkey(tup):
        name, func = tup
        return (func._priority, name)

    # Try autopull functions.
    funcs = [
        func
        for _name, func in sorted(_table.items(), key=lambda t: (t[1]._priority, t[0]))
    ]
    if not funcs:
        return

    # Collect all attempts.
    attempts = []
    for x in xs:
        for func in funcs:
            attempt = func(repo, x)
            if attempt:
                assert isinstance(attempt, pullattempt)
                attempts.append(attempt)

    # Merge all pullattempts and execute it.
    if attempts:
        attempt = attempts[0]
        for other in attempts[1:]:
            attempt.merge(other)
        attempt.execute(repo)
        unfi = repo.unfiltered()
        return all(x in unfi for x in xs)

    return False


@builtinautopullpredicate("remotenames", priority=10)
def _pullremotebookmarks(repo, x):
    # Pull remote names like "remote/foo" automatically.
    pattern = repo.ui.config("remotenames", "autopullpattern")
    if pattern and "/" in x:
        matchfn = _cachedstringmatcher(pattern)
        if matchfn(x):
            _remotename, name = bookmarks.splitremotename(x)
            # XXX: remotename should be the source, but is ignored here. This
            # is because "remote/scratch/x" might require special remote name
            # (ex.  "infinitepushbookmark", not "remote"/"default") to handle.
            return pullattempt(bookmarknames=[name], headnames=[name])


@builtinautopullpredicate("commits", priority=20)
def _pullcommits(repo, x):
    # Pull commit hashes automatically.
    if repo.ui.configbool("ui", "autopullcommits") and _commithashre.match(x):
        return pullattempt(headnames=[x])


@builtinautopullpredicate("hoistednames", priority=30)
def _pullhoistnames(repo, x):
    # Pull hoist remote names automatically. For example, "foo" -> "remote/foo".
    hoistpattern = repo.ui.config("remotenames", "autopullhoistpattern")
    if hoistpattern:
        matchfn = _cachedstringmatcher(hoistpattern)
        if matchfn(x):
            # XXX: remotenames.hoist config should be the "source" but is
            # ignored here. See "_pullremotebookmarks" for reasons.
            return pullattempt(bookmarknames=[x])


def loadpredicate(ui, extname, registrarobj):
    for name, func in pycompat.iteritems(registrarobj._table):
        if name in _table:
            raise error.ProgrammingError("namespace '%s' is already registered", name)
        _table[name] = func
