# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# copies.py - copy detection for Mercurial
#
# Copyright 2008 Olivia Mackall <olivia@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from . import git, node, pathutil, pycompat, util


def _findlimit(repo, a, b):
    """
    Find the earliest revision that's an ancestor of a or b but not both, except
    in the case where a or b is an ancestor of the other.
    """
    if a is None:
        a = repo.revs("p1()").first()
    if b is None:
        b = repo.revs("p1()").first()
    if a is None or b is None or not repo.revs("ancestor(%d, %d)", a, b):
        return None

    return repo.revs("only(%d, %d) + only(%d, %d) + %d + %d", a, b, b, a, a, b).min()


def _chain(src, dst, a, b):
    """chain two sets of copies a->b

    Assuming we have a commit graph like below::

        dst src
         | /
         |/
        base

    then:

    * `a` is a dict from `base` to `src`
    * `b` is a dict from `dst` to `base`

    This function returns a dict from `dst` to `src`.

    For example:
    * a is {"a": "x"}  # src rename a -> x
    * b is {"y": "a"}  # dst rename a -> y

    then the result will be {"y": "x"}
    """
    t = a.copy()
    for k, v in pycompat.iteritems(b):
        if v in t:
            # found a chain
            if t[v] != k:
                # file wasn't renamed back to itself
                t[k] = t[v]
            if v not in dst:
                # chain was a rename, not a copy
                del t[v]
        if v in src:
            # file is a copy of an existing file
            t[k] = v

    # remove criss-crossed copies
    for k, v in list(t.items()):
        if k in src and v in dst:
            del t[k]

    return t


def _dirstatecopies(d, match=None):
    ds = d._repo.dirstate
    c = ds.copies().copy()
    for k in list(c):
        if ds[k] not in "anm" or (match and not match(k)):
            del c[k]
    return c


def _reverse_copies(copies):
    """reverse the direction of the copies"""
    # For 1:n rename situations (e.g. hg cp a b; hg mv a c), we
    # arbitrarily pick one of the renames.
    return {v: k for k, v in copies.items()}


def pathcopies(x, y, match=None):
    """find {dst@y: src@x} copy mapping for directed compare"""
    if x == y or not x or not y:
        return {}

    dagcopytrace = y.repo()._dagcopytrace
    if y.rev() is None:
        dirstate_copies = _dirstatecopies(y, match)
        if x == y.p1():
            return dirstate_copies
        committed_copies = dagcopytrace.path_copies(x.node(), y.p1().node(), match)
        return _chain(x, y, committed_copies, dirstate_copies)

    if x.rev() is None:
        dirstate_copies = _reverse_copies(_dirstatecopies(x, match))
        if y == x.p1():
            return dirstate_copies
        committed_copies = dagcopytrace.path_copies(x.p1().node(), y.node(), match)
        return _chain(x, y, dirstate_copies, committed_copies)

    return dagcopytrace.path_copies(x.node(), y.node(), match)


def mergecopies(repo, c1, c2, base):
    # This function is wrapped by copytrace.mergecopies,
    return {}, {}, {}, {}, {}


def duplicatecopies(repo, wctx, rev, fromrev, skiprev=None):
    """reproduce copies from fromrev to rev in the dirstate

    If skiprev is specified, it's a revision that should be used to
    filter copy records. Any copies that occur between fromrev and
    skiprev will not be duplicated, even if they appear in the set of
    copies between fromrev and rev.
    """
    dagcopytrace = _get_dagcopytrace(repo, wctx, skiprev)
    for dst, src in pycompat.iteritems(pathcopies(repo[fromrev], repo[rev])):
        if (
            dagcopytrace
            and dst in repo[skiprev]
            and dagcopytrace.trace_rename(
                repo[skiprev].node(), repo[fromrev].node(), dst
            )
        ):
            continue
        wctx[dst].markcopied(src)


def _get_dagcopytrace(repo, wctx, skiprev):
    """this is for fixing empty commit issue in non-IMM case"""
    if (
        skiprev is None
        or wctx.isinmemory()
        or not repo.ui.configbool("copytrace", "skipduplicatecopies")
    ):
        return None
    return repo._dagcopytrace
