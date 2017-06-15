# obsutil.py - utility functions for obsolescence
#
# Copyright 2017 Boris Feld <boris.feld@octobus.net>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

def closestpredecessors(repo, nodeid):
    """yield the list of next predecessors pointing on visible changectx nodes

    This function respect the repoview filtering, filtered revision will be
    considered missing.
    """

    precursors = repo.obsstore.precursors
    stack = [nodeid]
    seen = set(stack)

    while stack:
        current = stack.pop()
        currentpreccs = precursors.get(current, ())

        for prec in currentpreccs:
            precnodeid = prec[0]

            # Basic cycle protection
            if precnodeid in seen:
                continue
            seen.add(precnodeid)

            if precnodeid in repo:
                yield precnodeid
            else:
                stack.append(precnodeid)
