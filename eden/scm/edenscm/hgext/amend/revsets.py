# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# revsets.py - revset definitions

from __future__ import absolute_import

from edenscm.mercurial import mutation, obsutil, phases, registrar, revset, smartset
from edenscm.mercurial.node import nullrev


revsetpredicate = registrar.revsetpredicate()


@revsetpredicate("_destrestack(SRC)")
def _destrestack(repo, subset, x):
    """restack destination for given single source revision"""
    unfi = repo.unfiltered()
    obsoleted = unfi.revs("obsolete()")
    getparents = unfi.changelog.parentrevs
    getphase = unfi._phasecache.phase
    nodemap = unfi.changelog.nodemap

    src = revset.getset(repo, subset, x).first()

    # Empty src or already obsoleted - Do not return a destination
    if not src or src in obsoleted:
        return smartset.baseset()

    # Find the obsoleted "base" by checking source's parent recursively
    base = src
    while base not in obsoleted:
        base = getparents(base)[0]
        # When encountering a public revision which cannot be obsoleted, stop
        # the search early and return no destination. Do the same for nullrev.
        if getphase(repo, base) == phases.public or base == nullrev:
            return smartset.baseset()

    # Find successors for given base
    # NOTE: Ideally we can use obsutil.successorssets to detect divergence
    # case. However it does not support cycles (unamend) well. So we use
    # allsuccessors and pick non-obsoleted successors manually as a workaround.
    basenode = repo[base].node()
    if mutation.enabled(repo):
        succnodes = mutation.allsuccessors(repo, [basenode])
    else:
        succnodes = obsutil.allsuccessors(repo.obsstore, [basenode])
    succnodes = [
        n
        for n in succnodes
        if (n != basenode and n in nodemap and nodemap[n] not in obsoleted)
    ]

    # In case of a split, only keep its heads
    succrevs = list(unfi.revs("heads(%ln)", succnodes))

    if len(succrevs) == 0:
        # Prune - Find the first non-obsoleted ancestor
        while base in obsoleted:
            base = getparents(base)[0]
            if base == nullrev:
                # Root node is pruned. The new base (destination) is the
                # virtual nullrev.
                return smartset.baseset([nullrev])
        return smartset.baseset([base])
    elif len(succrevs) == 1:
        # Unique visible successor case - A valid destination
        return smartset.baseset([succrevs[0]])
    else:
        # Multiple visible successors - Choose the one with a greater revision
        # number. This is to be compatible with restack old behavior. We might
        # want to revisit it when we introduce the divergence concept to users.
        return smartset.baseset([max(succrevs)])
