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
_table = {}  # {name: (repo, name) -> bool}

builtinautopullpredicate = registrar.autopullpredicate(_table)


def _cachedstringmatcher(pattern, _cache={}):
    # _cache is shared across function calls
    result = _cache.get(pattern)
    if result is None:
        result = util.stringmatcher(pattern)[-1]
        _cache[pattern] = result
    return result


def trypull(repo, x):
    """Pull the given name x. Return true if pull succeeded. Does not raise."""
    repo._autopulled = getattr(repo, "_autopulled", set())
    if x in repo._autopulled:
        # Do not attempt to pull the same name twice.
        return False
    repo._autopulled.add(x)

    # If paths.default is not set. Do not attempt to pull.
    if repo.ui.paths.get("default") is None:
        return False

    def sortkey(tup):
        name, func = tup
        return (func._priority, name)

    # Try autopull functions.
    for _name, func in sorted(_table.items(), key=lambda t: (t[1]._priority, t[0])):
        if func(repo, x):
            if x in repo.unfiltered():
                return True
    return False


@builtinautopullpredicate("builtin", priority=10)
def _builtinautopull(repo, x):
    def _trypull(source, bookmarknames=(), headnames=()):
        """Attempt to pull from source. Writes to ui.ferr. Returns True on success."""
        try:
            path = repo.ui.paths.getpath(source)
        except error.RepoError:
            # path does not exist. Skip
            return False

        url = str(path.url)

        if bookmarknames:
            displayname = _("bookmark %r") % ", ".join(bookmarknames)
            assert not headnames or headnames == bookmarknames
        else:
            displayname = ", ".join(headnames)
        repo.ui.status_err(_("pulling %s from %r\n") % (displayname, url))

        # When pulling a single commit. Also pull selectivepull bookmarks so
        # it does not end up with lagged master issues.
        if not bookmarknames and headnames:
            bookmarknames = bookmarks.selectivepullbookmarknames(repo, source)

        try:
            repo.pull(source, bookmarknames=bookmarknames, headnames=headnames)
        except Exception as ex:
            repo.ui.status_err(_("pull failed: %s\n") % ex)
            return False

        # Double-check that the names are actually pulled. This is needed
        # because the pull API does not make sure the names will become
        # resolvable (ex. it ignores bookmarks that do not exist, and uses
        # "lookup" to resolve names to hashes without storing the names)
        if x not in repo.unfiltered():
            return False

        return True

    # TODO: Once Mononoke handles all infinitepush pull requests, remove
    # _trypulls and just use a single path paths.default for pulling.
    def _trypulls(sources, bookmarknames=(), headnames=()):
        """Attempt to pull from the given sources. Remove this method once
        Mononoke serves all pull requests via the default path.
        """
        for source in sources:
            if _trypull(source, bookmarknames, headnames):
                return True
        return False

    # Pull remote names like "remote/foo" automatically.
    pattern = repo.ui.config("remotenames", "autopullpattern")
    if pattern and "/" in x:
        matchfn = _cachedstringmatcher(pattern)
        if matchfn(x):
            remotename, name = bookmarks.splitremotename(x)
            if _trypulls(
                ["infinitepushbookmark", "infinitepush", remotename],
                bookmarknames=[name],
                headnames=[name],
            ):
                return True

    # Pull commit hashes automatically.
    if repo.ui.configbool("ui", "autopullcommits") and _commithashre.match(x):
        if _trypulls(["infinitepush", "default"], headnames=[x]):
            return True

    # Pull hoist remote names automatically. For example, "foo" -> "remote/foo".
    hoistpattern = repo.ui.config("remotenames", "autopullhoistpattern")
    if hoistpattern:
        hoist = repo.ui.config("remotenames", "hoist")
        matchfn = _cachedstringmatcher(hoistpattern)
        if matchfn(x) and _trypulls(
            ["infinitepushbookmark", "infinitepush", hoist], bookmarknames=[x]
        ):
            return True

    return False


def loadpredicate(ui, extname, registrarobj):
    for name, func in pycompat.iteritems(registrarobj._table):
        if name in _table:
            raise error.ProgrammingError("namespace '%s' is already registered", name)
        _table[name] = func
