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
from mercurial.cmdutil import revrange

CHANGESET = 'C'

def revisions(repo, start, end):
    """DAG generator for revisions between start and end
    """
    revset = '%s:%s' % (start, end)
    return dagwalker(repo, revrange(repo, [revset]))

def filerevs(repo, path, start, stop, limit=None):
    """DAG generator, which is limited by file passed
    """
    revset = '%s:%s and file("%s")' % (start, stop, path)
    if limit:
        revset = 'limit(%s, %s)' % (revset, limit)
    return dagwalker(repo, revrange(repo, [revset]))

def dagwalker(repo, revs):
    """cset DAG generator yielding (id, CHANGESET, ctx, [parentids]) tuples

    This generator function walks through revisions (which should be ordered
    from bigger to lower). It returns a tuple for each node. The node and parent
    ids are arbitrary integers which identify a node in the context of the graph
    returned.
    """
    if not revs:
        return []

    ns = [repo[r].node() for r in revs]
    revdag = list(nodes(repo, ns))

    cl = repo.changelog
    lowestrev = min(revs)
    gpcache = {}
    leafs = {}

    for i, (id, c, ctx, parents) in enumerate(revdag):
        mpars = [p.rev() for p in ctx.parents() if
                 p.rev() != nullrev and p.rev() not in parents]
        grandparents = []

        for mpar in mpars:
            gp = gpcache.get(mpar) or grandparent(cl, lowestrev, revs, mpar)
            gpcache[mpar] = gp
            if gp is None:
                leafs.setdefault(mpar, []).append((i, ctx))
            else:
                grandparents.append(gp)

        if grandparents:
            for gp in grandparents:
                if gp not in revdag[i][3]:
                    revdag[i][3].append(gp)

    for parent, leafs in leafs.iteritems():
        for i, ctx in leafs:
            revdag[i][3].append(parent)

    return revdag

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
    """Return closest 'root' rev in topological path from 'roots' to 'head'.

    Derived from revlog.revlog.nodesbetween, but only returns next rev
    of topologically sorted list of all nodes N that satisfy of these
    constraints:

    1. N is a descendant of some node in 'roots'
    2. N is an ancestor of 'head'
    3. N is some node in 'roots' or nullrev

    Every node is considered to be both a descendant and an ancestor
    of itself, so every reachable node in 'roots' and 'head' will be
    included in 'nodes'.
    """
    ancestors = set()
    # Start at the top and keep marking parents until we're done.
    revstotag = set([head])
    revstotag.discard(nullrev)
    llowestrev = max(nullrev, lowestrev)

    while revstotag:
        r = revstotag.pop()
        # A node's revision number represents its place in a
        # topologically sorted list of nodes.
        if r >= llowestrev:
            if r not in ancestors:
                # If we are possibly a descendent of one of the roots
                # and we haven't already been marked as an ancestor
                ancestors.add(r) # Mark as ancestor
                # Add non-nullrev parents to list of nodes to tag.
                revstotag.update([p for p in cl.parentrevs(r)])

    if not ancestors:
        return
    # Now that we have our set of ancestors, we want to remove any
    # roots that are not ancestors.

    # If one of the roots was nullrev, everything is included anyway.
    if lowestrev > nullrev:
        # But, since we weren't, let's recompute the lowest rev to not
        # include roots that aren't ancestors.

        # Filter out roots that aren't ancestors of heads
        _roots = ancestors.intersection(roots)
        if not _roots:
            return
        # Recompute the lowest revision
        lowestrev = min(roots)
    else:
        # We are descending from nullrev, and don't need to care about
        # any other roots.
        lowestrev = nullrev
        _roots = [nullrev]

    # The roots are just the descendants.
    # Don't start at nullrev since we don't want nullrev in our output list,
    # and if nullrev shows up in descedents, empty parents will look like
    # they're descendents.
    lowestrevisnullrev = (lowestrev == nullrev)
    for r in xrange(head - 1, max(lowestrev, -1) - 1, -1):
        if lowestrevisnullrev or r in _roots:
            pass
        elif _roots.issuperset(cl.parentrevs(r)):
            # A node is a descendent if either of its parents are
            # descendents.  (We seeded the dependents list with the roots
            # up there, remember?)
            _roots.add(r)
        else:
            continue
        if r in ancestors:
            # Only include nodes that are both descendents and ancestors.
            return r
