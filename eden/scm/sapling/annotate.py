# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Copyright Mercurial Contributors
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from typing import Callable, List, Tuple, TypeVar

from . import error, mdiff


class annotateline:
    def __init__(self, fctx=None, ctx=None, lineno=None, path=None):
        if (not fctx) == (not ctx):
            raise error.ProgrammingError("must specify exactly one of ctx or fctx")
        if not fctx and not path:
            raise error.ProgrammingError("must specify fctx or path")

        self._ctx = ctx or fctx.changectx()
        self._fctx = fctx
        self._path = path or fctx.path()
        self.lineno = lineno

    def ctx(self):
        return self._ctx

    def date(self):
        # Prefer fctx.date() since that can differ for wdir files.
        return (self._fctx or self._ctx).date()

    def rev(self):
        return self._ctx.rev()

    def node(self):
        return self._ctx.node()

    def path(self):
        return self._path

    def user(self):
        return self._ctx.user()

    def origin_url(self):
        return self._ctx.repo().origin_url()


F = TypeVar("F")
L = TypeVar("L")


def annotate(
    base: F,
    parents: Callable[[F], List[F]],
    decorate: Callable[[F], Tuple[List[L], bytes]],
    diffopts: mdiff.diffopts,
) -> Tuple[List[L], bytes]:
    """annotate algorithm

    base: starting point, usually a fctx.
    parents: get parents from F.
    decorate: get (lines, text) from F.

    Return (lines, text) for 'base'.
    """
    # This algorithm would prefer to be recursive, but Python is a
    # bit recursion-hostile. Instead we do an iterative
    # depth-first search.

    # 1st DFS pre-calculates pcache and needed
    visit = [base]
    pcache = {}
    needed = {base: 1}
    while visit:
        f = visit.pop()
        if f in pcache:
            continue
        pl = parents(f)
        pcache[f] = pl
        for p in pl:
            needed[p] = needed.get(p, 0) + 1
            if p not in pcache:
                visit.append(p)

    # 2nd DFS does the actual annotate
    visit[:] = [base]
    hist = {}
    while visit:
        f = visit[-1]
        if f in hist:
            visit.pop()
            continue

        ready = True
        pl = pcache[f]
        for p in pl:
            if p not in hist:
                ready = False
                visit.append(p)
        if ready:
            visit.pop()
            curr = decorate(f)
            curr = annotatepair([hist[p] for p in pl], curr, diffopts)
            for p in pl:
                if needed[p] == 1:
                    del hist[p]
                    del needed[p]
                else:
                    needed[p] -= 1

            hist[f] = curr
            del pcache[f]

    return hist[base]


def annotatepair(parents, child, diffopts):
    r"""
    Given parent and child fctxes and annotate data for parents, for all lines
    in either parent that match the child, annotate the child with the parent's
    data.

    See test-annotate.py for unit tests.
    """
    if not parents:
        return child

    pblocks = [
        (parent, mdiff.allblocks(parent[1], child[1], opts=diffopts))
        for parent in parents
    ]

    # Mercurial currently prefers p2 over p1 for annotate.
    # TODO: change this?
    for parent, blocks in pblocks:
        for (a1, a2, b1, b2), t in blocks:
            # Changed blocks ('!') or blocks made only of blank lines ('~')
            # belong to the child.
            if t == "=":
                child[0][b1:b2] = parent[0][a1:a2]

    return child


def create_line_decorator(linenumber: bool) -> Callable[[F], Tuple[List[L], bytes]]:
    """Create a decorator for annotate() function."""

    def lines(text):
        if text.endswith(b"\n"):
            return text.count(b"\n")
        return text.count(b"\n") + int(bool(text))

    if linenumber:

        def decorate(fctx):
            text = fctx.data()
            return (
                [annotateline(fctx=fctx, lineno=i) for i in range(1, lines(text) + 1)],
                text,
            )
    else:

        def decorate(fctx):
            text = fctx.data()
            return ([annotateline(fctx=fctx)] * lines(text), text)

    return decorate
