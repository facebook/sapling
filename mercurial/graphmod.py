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

from mercurial.node import nullrev

CHANGESET = 'C'

def dagwalker(repo, revs):
    """cset DAG generator yielding (id, CHANGESET, ctx, [parentids]) tuples

    This generator function walks through revisions (which should be ordered
    from bigger to lower). It returns a tuple for each node. The node and parent
    ids are arbitrary integers which identify a node in the context of the graph
    returned.
    """
    if not revs:
        return

    cl = repo.changelog
    lowestrev = min(revs)
    gpcache = {}

    knownrevs = set(revs)
    for rev in revs:
        ctx = repo[rev]
        parents = sorted(set([p.rev() for p in ctx.parents()
                              if p.rev() in knownrevs]))
        mpars = [p.rev() for p in ctx.parents() if
                 p.rev() != nullrev and p.rev() not in parents]

        for mpar in mpars:
            gp = gpcache.get(mpar)
            if gp is None:
                gp = gpcache[mpar] = grandparent(cl, lowestrev, revs, mpar)
            if not gp:
                parents.append(mpar)
            else:
                parents.extend(g for g in gp if g not in parents)

        yield (ctx.rev(), CHANGESET, ctx, parents)

def nodes(repo, nodes):
    """cset DAG generator yielding (id, CHANGESET, ctx, [parentids]) tuples

    This generator function walks the given nodes. It only returns parents
    that are in nodes, too.
    """
    include = set(nodes)
    for node in nodes:
        ctx = repo[node]
        parents = set([p.rev() for p in ctx.parents() if p.node() in include])
        yield (ctx.rev(), CHANGESET, ctx, sorted(parents))

def colored(dag):
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
    for (cur, type, data, parents) in dag:

        # Compute seen and next
        if cur not in seen:
            seen.append(cur) # new head
            colors[cur] = newcolor
            newcolor += 1

        col = seen.index(cur)
        color = colors.pop(cur)
        next = seen[:]

        # Add parents to next
        addparents = [p for p in parents if p not in next]
        next[col:col + 1] = addparents

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
                edges.append((ecol, next.index(eid), colors[eid]))
            elif eid == cur:
                for p in parents:
                    edges.append((ecol, next.index(p), color))

        # Yield and move on
        yield (cur, type, data, (col, color), edges)
        seen = next

def grandparent(cl, lowestrev, roots, head):
    """Return all ancestors of head in roots which revision is
    greater or equal to lowestrev.
    """
    pending = set([head])
    seen = set()
    kept = set()
    llowestrev = max(nullrev, lowestrev)
    while pending:
        r = pending.pop()
        if r >= llowestrev and r not in seen:
            if r in roots:
                kept.add(r)
            else:
                pending.update([p for p in cl.parentrevs(r)])
            seen.add(r)
    return sorted(kept)
