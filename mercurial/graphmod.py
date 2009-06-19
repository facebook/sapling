# Revision graph generator for Mercurial
#
# Copyright 2008 Dirkjan Ochtman <dirkjan@ochtman.nl>
# Copyright 2007 Joel Rosdahl <joel@rosdahl.net>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2, incorporated herein by reference.

from node import nullrev

def revisions(repo, start, stop):
    """cset DAG generator yielding (rev, node, [parents]) tuples

    This generator function walks through the revision history from revision
    start to revision stop (which must be less than or equal to start).
    """
    assert start >= stop
    cur = start
    while cur >= stop:
        ctx = repo[cur]
        parents = [p.rev() for p in ctx.parents() if p.rev() != nullrev]
        parents.sort()
        yield (ctx, parents)
        cur -= 1

def filerevs(repo, path, start, stop):
    """file cset DAG generator yielding (rev, node, [parents]) tuples

    This generator function walks through the revision history of a single
    file from revision start to revision stop (which must be less than or
    equal to start).
    """
    assert start >= stop
    filerev = len(repo.file(path)) - 1
    while filerev >= 0:
        fctx = repo.filectx(path, fileid=filerev)
        parents = [f.linkrev() for f in fctx.parents() if f.path() == path]
        parents.sort()
        if fctx.rev() <= start:
            yield (fctx, parents)
        if fctx.rev() <= stop:
            break
        filerev -= 1

def nodes(repo, nodes):
    include = set(nodes)
    for node in nodes:
        ctx = repo[node]
        parents = [p.rev() for p in ctx.parents() if p.node() in include]
        parents.sort()
        yield (ctx, parents)

def graph(repo, start_rev, stop_rev):
    """incremental revision grapher

    This generator function walks through the revision history from
    revision start_rev to revision stop_rev (which must be less than
    or equal to start_rev) and for each revision emits tuples with the
    following elements:

      - Context of the current node
      - Tuple (col, color) with column and color index for the current node
      - Edges; a list of (col, next_col, color) indicating the edges between
        the current node and its parents.
    """

    if start_rev == nullrev and not stop_rev:
        return

    assert start_rev >= stop_rev
    assert stop_rev >= 0
    curr_rev = start_rev
    revs = []
    cl = repo.changelog
    colors = {}
    new_color = 1

    while curr_rev >= stop_rev:
        # Compute revs and next_revs
        if curr_rev not in revs:
            revs.append(curr_rev) # new head
            colors[curr_rev] = new_color
            new_color += 1

        idx = revs.index(curr_rev)
        color = colors.pop(curr_rev)
        next = revs[:]

        # Add parents to next_revs
        parents = [x for x in cl.parentrevs(curr_rev) if x != nullrev]
        addparents = [p for p in parents if p not in next]
        next[idx:idx + 1] = addparents

        # Set colors for the parents
        for i, p in enumerate(addparents):
            if not i:
                colors[p] = color
            else:
                colors[p] = new_color
                new_color += 1

        # Add edges to the graph
        edges = []
        for col, r in enumerate(revs):
            if r in next:
                edges.append((col, next.index(r), colors[r]))
            elif r == curr_rev:
                for p in parents:
                    edges.append((col, next.index(p), colors[p]))

        # Yield and move on
        yield (repo[curr_rev], (idx, color), edges)
        revs = next
        curr_rev -= 1
