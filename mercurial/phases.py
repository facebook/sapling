# Mercurial phases support code
#
# Copyright 2011 Pierre-Yves David <pierre-yves.david@ens-lyon.org>
#                Logilab SA        <contact@logilab.fr>
#                Augie Fackler     <durin42@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import errno
from node import nullid, bin, hex, short
from i18n import _

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

def filterunknown(repo, phaseroots=None):
    """remove unknown nodes from the phase boundary

    no data is lost as unknown node only old data for their descentants
    """
    if phaseroots is None:
        phaseroots = repo._phaseroots
    for phase, nodes in enumerate(phaseroots):
        missing = [node for node in nodes if node not in repo]
        if missing:
            for mnode in missing:
                msg = _('Removing unknown node %(n)s from %(p)i-phase boundary')
                repo.ui.debug(msg, {'n': short(mnode), 'p': phase})
            nodes.symmetric_difference_update(missing)
            repo._dirtyphases = True

def advanceboundary(repo, targetphase, nodes):
    """Add nodes to a phase changing other nodes phases if necessary.

    This function move boundary *forward* this means that all nodes are set
    in the target phase or kept in a *lower* phase.

    Simplify boundary to contains phase roots only."""
    for phase in xrange(targetphase + 1, len(allphases)):
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

def retractboundary(repo, targetphase, nodes):
    """Set nodes back to a phase changing other nodes phases if necessary.

    This function move boundary *backward* this means that all nodes are set
    in the target phase or kept in a *higher* phase.

    Simplify boundary to contains phase roots only."""
    currentroots = repo._phaseroots[targetphase]
    newroots = [n for n in nodes if repo[n].phase() < targetphase]
    if newroots:
        currentroots.update(newroots)
        ctxs = repo.set('roots(%ln::)', currentroots)
        currentroots.intersection_update(ctx.node() for ctx in ctxs)
        if '_phaserev' in vars(repo):
            del repo._phaserev
        repo._dirtyphases = True


def listphases(repo):
    """List phases root for serialisation over pushkey"""
    keys = {}
    for phase in trackedphases:
        for root in repo._phaseroots[phase]:
            keys[hex(root)] = '%i' % phase
    if repo.ui.configbool('phases', 'publish', True):
        # Add an extra data to let remote know we are a publishing repo.
        # Publishing repo can't just pretend they are old repo. When pushing to
        # a publishing repo, the client still need to push phase boundary
        #
        # Push do not only push changeset. It also push phase data. New
        # phase data may apply to common changeset which won't be push (as they
        # are common).  Here is a very simple example:
        #
        # 1) repo A push changeset X as draft to repo B
        # 2) repo B make changeset X public
        # 3) repo B push to repo A. X is not pushed but the data that X as now
        #    public should
        #
        # The server can't handle it on it's own as it has no idea of client
        # phase data.
        keys['publishing'] = 'True'
    return keys

def pushphase(repo, nhex, oldphasestr, newphasestr):
    """List phases root for serialisation over pushkey"""
    lock = repo.lock()
    try:
        currentphase = repo[nhex].phase()
        newphase = abs(int(newphasestr)) # let's avoid negative index surprise
        oldphase = abs(int(oldphasestr)) # let's avoid negative index surprise
        if currentphase == oldphase and newphase < oldphase:
            advanceboundary(repo, newphase, [bin(nhex)])
            return 1
        else:
            return 0
    finally:
        lock.release()

def analyzeremotephases(repo, subset, roots):
    """Compute phases heads and root in a subset of node from root dict

    * subset is heads of the subset
    * roots is {<nodeid> => phase} mapping. key and value are string.

    Accept unknown element input
    """
    # build list from dictionary
    phaseroots = [[] for p in allphases]
    for nhex, phase in roots.iteritems():
        if nhex == 'publishing': # ignore data related to publish option
            continue
        node = bin(nhex)
        phase = int(phase)
        if node in repo:
            phaseroots[phase].append(node)
    # compute heads
    phaseheads = [[] for p in allphases]
    for phase in allphases[:-1]:
        toproof = phaseroots[phase + 1]
        revset = repo.set('heads((%ln + parents(%ln)) - (%ln::%ln))',
                          subset, toproof, toproof, subset)
        phaseheads[phase].extend(c.node() for c in revset)
    return phaseheads, phaseroots

