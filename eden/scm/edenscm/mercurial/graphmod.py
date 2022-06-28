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

from __future__ import absolute_import

from . import dagop, smartset, util
from .node import nullrev


CHANGESET = "C"
PARENT = "P"
GRANDPARENT = "G"
MISSINGPARENT = "M"
# Style of line to draw. None signals a line that ends and is removed at this
# point. A number prefix means only the last N characters of the current block
# will use that style, the rest will use the PARENT style. Add a - sign
# (so making N negative) and all but the first N characters use that style.
EDGES = {PARENT: "|", GRANDPARENT: ":", MISSINGPARENT: None}


def dagwalker(repo, revs, template):
    """cset DAG generator yielding (id, CHANGESET, ctx, [parentinfo]) tuples

    This generator function walks through revisions (which should be ordered
    from bigger to lower). It returns a tuple for each node.

    Each parentinfo entry is a tuple with (edgetype, parentid), where edgetype
    is one of PARENT, GRANDPARENT or MISSINGPARENT. The node and parent ids
    are arbitrary integers which identify a node in the context of the graph
    returned.

    """
    if not revs:
        return

    simplifygrandparents = repo.ui.configbool("log", "simplify-grandparents")
    cl = repo.changelog
    if cl.algorithmbackend != "segments":
        simplifygrandparents = False
    if simplifygrandparents:
        rootnodes = cl.tonodes(revs)

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
        parents = [(PARENT, p) for p in sorted(pset)]

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
                parents.append((MISSINGPARENT, mpar))
                pset.add(mpar)
            else:
                parents.extend((GRANDPARENT, g) for g in gp if g not in pset)
                pset.update(gp)

        yield (ctx.rev(), CHANGESET, ctx, parents)


def nodes(repo, nodes):
    """cset DAG generator yielding (id, CHANGESET, ctx, [parentids]) tuples

    This generator function walks the given nodes. It only returns parents
    that are in nodes, too.
    """
    include = set(nodes)
    for node in nodes:
        ctx = repo[node]
        parents = set((PARENT, p.rev()) for p in ctx.parents() if p.node() in include)
        yield (ctx.rev(), CHANGESET, ctx, sorted(parents))


def colored(dag, repo):
    """annotates a DAG with colored edge information

    For each DAG node this function emits tuples::

      (id, type, data, (col, color), [(col, nextcol, color)])

    with the following new elements:

      - Tuple (col, color) with column and color index for the current node
      - A list of tuples indicating the edges between the current node and its
        parents.
    """
    seen = []
    colors = {}
    newcolor = 1
    config = {}

    for key, val in repo.ui.configitems("graph"):
        if "." in key:
            branch, setting = key.rsplit(".", 1)
            # Validation
            if setting == "width" and val.isdigit():
                config.setdefault(branch, {})[setting] = int(val)
            elif setting == "color" and val.isalnum():
                config.setdefault(branch, {})[setting] = val

    if config:
        getconf = util.lrucachefunc(lambda rev: config.get(repo[rev].branch(), {}))
    else:
        getconf = lambda rev: {}

    for (cur, type, data, parents) in dag:

        # Compute seen and next
        if cur not in seen:
            seen.append(cur)  # new head
            colors[cur] = newcolor
            newcolor += 1

        col = seen.index(cur)
        color = colors.pop(cur)
        next = seen[:]

        # Add parents to next
        addparents = [p for pt, p in parents if p not in next]
        next[col : col + 1] = addparents

        # Set colors for the parents
        for i, p in enumerate(addparents):
            if not i:
                colors[p] = color
            else:
                colors[p] = newcolor
                newcolor += 1

        # Add edges to the graph
        edges = []
        for ecol, eid in enumerate(seen):
            if eid in next:
                bconf = getconf(eid)
                edges.append(
                    (
                        ecol,
                        next.index(eid),
                        colors[eid],
                        bconf.get("width", -1),
                        bconf.get("color", ""),
                    )
                )
            elif eid == cur:
                for ptype, p in parents:
                    bconf = getconf(p)
                    edges.append(
                        (
                            ecol,
                            next.index(p),
                            color,
                            bconf.get("width", -1),
                            bconf.get("color", ""),
                        )
                    )

        # Yield and move on
        yield (cur, type, data, (col, color), edges)
        seen = next
