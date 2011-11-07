# Mercurial phases support code
#
# Copyright 2011 Pierre-Yves David <pierre-yves.david@ens-lyon.org>
#                Logilab SA        <contact@logilab.fr>
#                Augie Fackler     <durin42@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import errno
from node import nullid, bin, hex

allphases = range(2)
trackedphases = allphases[1:]

def readroots(repo):
    """Read phase roots from disk"""
    roots = [set() for i in allphases]
    roots[0].add(nullid)
    try:
        f = repo.sopener('phaseroots')
        try:
            for line in f:
                phase, nh = line.strip().split()
                roots[int(phase)].add(bin(nh))
        finally:
            f.close()
    except IOError, inst:
        if inst.errno != errno.ENOENT:
            raise
    return roots

def writeroots(repo):
    """Write phase roots from disk"""
    f = repo.sopener('phaseroots', 'w', atomictemp=True)
    try:
        for phase, roots in enumerate(repo._phaseroots):
            for h in roots:
                f.write('%i %s\n' % (phase, hex(h)))
        repo._dirtyphases = False
    finally:
        f.close()

def moveboundary(repo, target_phase, nodes):
    """Add nodes to a phase changing other nodes phases if necessary.

    Simplify boundary to contains phase roots only."""

    # move roots of lower states
    for phase in xrange(target_phase + 1, len(allphases)):
        # filter nodes that are not in a compatible phase already
        # XXX rev phase cache might have been invalidated by a previous loop
        # XXX we need to be smarter here
        nodes = [n for n in nodes if repo[n].phase() >= phase]
        if not nodes:
            break # no roots to move anymore
        roots = repo._phaseroots[phase]
        olds = roots.copy()
        ctxs = list(repo.set('roots((%ln::) - (%ln::%ln))', olds, olds, nodes))
        roots.clear()
        roots.update(ctx.node() for ctx in ctxs)
        if olds != roots:
            # invalidate cache (we probably could be smarter here
            if '_phaserev' in vars(repo):
                del repo._phaserev
            repo._dirtyphases = True
