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

@revsetpredicate('successors(set)')
def successors(repo, subset, x):
    """Immediate successors for given set"""
    # getsuccessors: lookup node by precursor
    getsuccessors = repo.obsstore.successors.get
    def f(nodes):
        for n in nodes:
            for m in getsuccessors(n, ()):
                # m[0]: precursor, m[1]: successors
                for n in m[1]:
                    yield n
    return _calculateset(repo, subset, x, f)

@revsetpredicate('precursors(set)')
def precursors(repo, subset, x):
    """Immediate precursors for given set"""
    # getsuccessors: lookup node by precursor
    getprecursors = repo.obsstore.precursors.get
    def f(nodes):
        for n in nodes:
            for m in getprecursors(n, ()):
                # m[0]: precursor, m[1]: successors
                yield m[0]
    return _calculateset(repo, subset, x, f)

@revsetpredicate('allsuccessors(set)')
def allsuccessors(repo, subset, x):
    """All changesets which are successors for given set, recursively"""
    f = lambda nodes: obsutil.allsuccessors(repo.obsstore, nodes)
    return _calculateset(repo, subset, x, f)

@revsetpredicate('allprecursors(set)')
def allprecursors(repo, subset, x):
    """All changesets which are precursors for given set, recursively"""
    f = lambda nodes: obsutil.allprecursors(repo.obsstore, nodes)
    return _calculateset(repo, subset, x, f)
