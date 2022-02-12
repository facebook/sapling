# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Copyright Mercurial Contributors
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from typing import TypeVar, Callable, List, Tuple, Optional

from . import mdiff
from .thirdparty import attr

F = TypeVar("F")
L = TypeVar("L")


def annotate(
    base: F,
    parents: Callable[[F], List[F]],
    decorate: Callable[[F], Tuple[List[L], bytes]],
    diffopts: mdiff.diffopts,
    skip: Optional[Callable[[F], bool]] = None,
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
            skipchild = False
            if skip is not None:
                skipchild = skip(f)
            curr = _annotatepair([hist[p] for p in pl], f, curr, skipchild, diffopts)
            for p in pl:
                if needed[p] == 1:
                    del hist[p]
                    del needed[p]
                else:
                    needed[p] -= 1

            hist[f] = curr
            del pcache[f]

    return hist[base]


def _annotatepair(parents, childfctx, child, skipchild, diffopts):
    r"""
    Given parent and child fctxes and annotate data for parents, for all lines
    in either parent that match the child, annotate the child with the parent's
    data.

    Additionally, if `skipchild` is True, replace all other lines with parent
    annotate data as well such that child is never blamed for any lines.

    See test-annotate.py for unit tests.
    """
    pblocks = [
        (parent, mdiff.allblocks(parent[1], child[1], opts=diffopts))
        for parent in parents
    ]

    if skipchild:
        # Need to iterate over the blocks twice -- make it a list
        pblocks = [(p, list(blocks)) for (p, blocks) in pblocks]
    # Mercurial currently prefers p2 over p1 for annotate.
    # TODO: change this?
    for parent, blocks in pblocks:
        for (a1, a2, b1, b2), t in blocks:
            # Changed blocks ('!') or blocks made only of blank lines ('~')
            # belong to the child.
            if t == "=":
                child[0][b1:b2] = parent[0][a1:a2]

    if skipchild:
        # Now try and match up anything that couldn't be matched,
        # Reversing pblocks maintains bias towards p2, matching above
        # behavior.
        pblocks.reverse()

        # The heuristics are:
        # * Work on blocks of changed lines (effectively diff hunks with -U0).
        # This could potentially be smarter but works well enough.
        # * For a non-matching section, do a best-effort fit. Match lines in
        #   diff hunks 1:1, dropping lines as necessary.
        # * Repeat the last line as a last resort.

        # First, replace as much as possible without repeating the last line.
        remaining = [(parent, []) for parent, _blocks in pblocks]
        for idx, (parent, blocks) in enumerate(pblocks):
            for (a1, a2, b1, b2), _t in blocks:
                if a2 - a1 >= b2 - b1:
                    for bk in range(b1, b2):
                        if child[0][bk].fctx == childfctx:
                            ak = min(a1 + (bk - b1), a2 - 1)
                            child[0][bk] = attr.evolve(parent[0][ak], skip=True)
                else:
                    remaining[idx][1].append((a1, a2, b1, b2))

        # Then, look at anything left, which might involve repeating the last
        # line.
        for parent, blocks in remaining:
            for a1, a2, b1, b2 in blocks:
                for bk in range(b1, b2):
                    if child[0][bk].fctx == childfctx:
                        ak = min(a1 + (bk - b1), a2 - 1)
                        child[0][bk] = attr.evolve(parent[0][ak], skip=True)
    return child
