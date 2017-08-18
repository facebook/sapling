# revsets.py - revset definitions
#
# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from mercurial import (
    obsutil,
    registrar,
    revset,
    smartset,
)

revsetpredicate = registrar.revsetpredicate()

def _calculateset(repo, subset, x, f):
    """f is a function that converts input nodes to output nodes

    repo, subset, x are typical revsetpredicate parameters.

    This function takes care of converting between revs/nodes, and filtering.
    """
    revs = revset.getset(repo, revset.fullreposet(repo), x)
    cl = repo.unfiltered().changelog
    torev = cl.rev
    tonode = cl.node
    nodemap = cl.nodemap
    resultrevs = set(torev(n)
                     for n in f(tonode(r) for r in revs)
                     if n in nodemap)
    s = smartset.baseset(resultrevs - set(revs) - repo.changelog.filteredrevs)
    s.sort()
    return subset & s

@revsetpredicate('precursors(set)')
@revsetpredicate('predecessors(set)')
def predecessors(repo, subset, x):
    """Immediate predecessors for given set"""
    getpredecessors = repo.obsstore.predecessors.get
    def f(nodes):
        for n in nodes:
            for m in getpredecessors(n, ()):
                # m[0]: predecessor, m[1]: successors
                yield m[0]
    return _calculateset(repo, subset, x, f)

@revsetpredicate('allsuccessors(set)')
def allsuccessors(repo, subset, x):
    """All changesets which are successors for given set, recursively"""
    f = lambda nodes: obsutil.allsuccessors(repo.obsstore, nodes)
    return _calculateset(repo, subset, x, f)

@revsetpredicate('allprecursors(set)')
@revsetpredicate('allpredecessors(set)')
def allpredecessors(repo, subset, x):
    """All changesets which are predecessors for given set, recursively"""
    f = lambda nodes: obsutil.allpredecessors(repo.obsstore, nodes)
    return _calculateset(repo, subset, x, f)
