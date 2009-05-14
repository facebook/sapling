# Revision graph generator for Mercurial
#
# Copyright 2008 Dirkjan Ochtman <dirkjan@ochtman.nl>
# Copyright 2007 Joel Rosdahl <joel@rosdahl.net>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2, incorporated herein by reference.

from node import nullrev

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
