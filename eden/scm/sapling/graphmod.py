# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Revision graph generator for Mercurial
#
# Copyright 2008 Dirkjan Ochtman <dirkjan@ochtman.nl>
# Copyright 2007 Joel Rosdahl <joel@rosdahl.net>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""supports walking the history as DAGs suitable for graphical output

The most basic format we use is that of::

  (id, type, data, [parentids])

The node and parent ids are arbitrary integers which identify a node in the
context of the graph returned. Type is a constant specifying the node type.
Data depends on type.
"""

from . import dagop, smartset
from .node import nullrev

CHANGESET = "C"
PARENT = "P"
GRANDPARENT = "G"
XREPOPARENT = "X"
MISSINGPARENT = "M"
# Style of line to draw. None signals a line that ends and is removed at this
# point. A number prefix means only the last N characters of the current block
# will use that style, the rest will use the PARENT style. Add a - sign
# (so making N negative) and all but the first N characters use that style.
EDGES = {PARENT: "|", GRANDPARENT: ":", MISSINGPARENT: None}


def dagwalker(repo, revs, template, idfunc=None):
    """cset DAG generator yielding (id, CHANGESET, ctx, [parentinfo]) tuples

    This generator function walks through revisions (which should be ordered
    from bigger to lower). It returns a tuple for each node.

    Each parentinfo entry is a tuple with (edgetype, parentid), where edgetype
    is one of PARENT, GRANDPARENT or MISSINGPARENT. The node and parent ids
    are arbitrary integers which identify a node in the context of the graph
    returned.

    The idfunc is a function that takes a rev number and returns an id for it.
    """
    if not revs:
        return

    simplifygrandparents = repo.ui.configbool("log", "simplify-grandparents")
    cl = repo.changelog
    if cl.algorithmbackend != "segments":
        simplifygrandparents = False
    if simplifygrandparents:
        rootnodes = cl.tonodes(revs)

    if idfunc is None:
        idfunc = lambda rev: rev

    gpcache = {}
    ctxstream = revs.prefetchbytemplate(repo, template).iterctx()
    for ctx in ctxstream:
        # partition into parents in the rev set and missing parents, then
        # augment the lists with markers, to inform graph drawing code about
        # what kind of edge to draw between nodes.
        pset = set(p.rev() for p in ctx.parents() if p.rev() in revs)
        mpars = [
            p.rev() for p in ctx.parents() if p.rev() != nullrev and p.rev() not in pset
        ]
        parents = [(PARENT, idfunc(p)) for p in sorted(pset)]

        for mpar in mpars:
            gp = gpcache.get(mpar)
            if gp is None:
                if simplifygrandparents:
                    gp = gpcache[mpar] = cl.torevs(
                        cl.dageval(
                            lambda: headsancestors(
                                ancestors(cl.tonodes([mpar])) & rootnodes
                            )
                        )
                    )

                else:
                    # precompute slow query as we know reachableroots() goes
                    # through all revs (issue4782)
                    if not isinstance(revs, smartset.baseset):
                        revs = smartset.baseset(revs, repo=repo)
                    gp = gpcache[mpar] = sorted(
                        set(dagop.reachableroots(repo, revs, [mpar]))
                    )
            if not gp:
                parents.append((MISSINGPARENT, idfunc(mpar)))
                pset.add(mpar)
            else:
                parents.extend((GRANDPARENT, idfunc(g)) for g in gp if g not in pset)
                pset.update(gp)

        yield (idfunc(ctx.rev()), CHANGESET, ctx, parents)
