# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Copyright Mercurial Contributors
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from typing import Callable, List, Tuple, TypeVar

from . import mdiff

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
            curr = _annotatepair([hist[p] for p in pl], curr, diffopts)
            for p in pl:
                if needed[p] == 1:
                    del hist[p]
                    del needed[p]
                else:
                    needed[p] -= 1

            hist[f] = curr
            del pcache[f]

    return hist[base]


def _annotatepair(parents, child, diffopts):
    r"""
    Given parent and child fctxes and annotate data for parents, for all lines
    in either parent that match the child, annotate the child with the parent's
    data.

    See test-annotate.py for unit tests.
    """
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
