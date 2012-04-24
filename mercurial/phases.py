""" Mercurial phases support code

    ---

    Copyright 2011 Pierre-Yves David <pierre-yves.david@ens-lyon.org>
                   Logilab SA        <contact@logilab.fr>
                   Augie Fackler     <durin42@gmail.com>

    This software may be used and distributed according to the terms of the
    GNU General Public License version 2 or any later version.

    ---

This module implements most phase logic in mercurial.


Basic Concept
=============

A 'changeset phases' is an indicator that tells us how a changeset is
manipulated and communicated. The details of each phase is described below,
here we describe the properties they have in common.

Like bookmarks, phases are not stored in history and thus are not permanent and
leave no audit trail.

First, no changeset can be in two phases at once. Phases are ordered, so they
can be considered from lowest to highest. The default, lowest phase is 'public'
- this is the normal phase of existing changesets. A child changeset can not be
in a lower phase than its parents.

These phases share a hierarchy of traits:

            immutable shared
    public:     X        X
    draft:               X
    secret:

local commits are draft by default

Phase movement and exchange
============================

Phase data are exchanged by pushkey on pull and push. Some server have a
publish option set, we call them publishing server. Pushing to such server make
draft changeset publish.

A small list of fact/rules define the exchange of phase:

* old client never changes server states
* pull never changes server states
* publish and old server csets are seen as public by client

* Any secret changeset seens in another repository is lowered to at least draft


Here is the final table summing up the 49 possible usecase of phase exchange:

                           server
                  old     publish      non-publish
                 N   X    N   D   P    N   D   P
    old client
    pull
     N           -   X/X  -   X/D X/P  -   X/D X/P
     X           -   X/X  -   X/D X/P  -   X/D X/P
    push
     X           X/X X/X  X/P X/P X/P  X/D X/D X/P
    new client
    pull
     N           -   P/X  -   P/D P/P  -   D/D P/P
     D           -   P/X  -   P/D P/P  -   D/D P/P
     P           -   P/X  -   P/D P/P  -   P/D P/P
    push
     D           P/X P/X  P/P P/P P/P  D/D D/D P/P
     P           P/X P/X  P/P P/P P/P  P/P P/P P/P

Legend:

    A/B = final state on client / state on server

    * N = new/not present,
    * P = public,
    * D = draft,
    * X = not tracked (ie: the old client or server has no internal way of
          recording the phase.)

    passive = only pushes


    A cell here can be read like this:

    "When a new client pushes a draft changeset (D) to a publishing server
    where it's not present (N), it's marked public on both sides (P/P)."

Note: old client behave as publish server with Draft only content
- other people see it as public
- content is pushed as draft

"""

import errno
from node import nullid, bin, hex, short
from i18n import _

allphases = public, draft, secret = range(3)
trackedphases = allphases[1:]
phasenames = ['public', 'draft', 'secret']

def readroots(repo):
    """Read phase roots from disk"""
    roots = [set() for i in allphases]
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
        for f in repo._phasedefaults:
            roots = f(repo, roots)
        repo._dirtyphases = True
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
    nodemap = repo.changelog.nodemap # to filter unknown nodes
    for phase, nodes in enumerate(phaseroots):
        missing = [node for node in nodes if node not in nodemap]
        if missing:
            for mnode in missing:
                repo.ui.debug(
                    'removing unknown node %s from %i-phase boundary\n'
                    % (short(mnode), phase))
            nodes.symmetric_difference_update(missing)
            repo._dirtyphases = True

def advanceboundary(repo, targetphase, nodes):
    """Add nodes to a phase changing other nodes phases if necessary.

    This function move boundary *forward* this means that all nodes are set
    in the target phase or kept in a *lower* phase.

    Simplify boundary to contains phase roots only."""
    delroots = [] # set of root deleted by this path
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
            # some roots may need to be declared for lower phases
            delroots.extend(olds - roots)
        # declare deleted root in the target phase
        if targetphase != 0:
            retractboundary(repo, targetphase, delroots)


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
    value = '%i' % draft
    for root in repo._phaseroots[draft]:
        keys[hex(root)] = value

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
        elif currentphase == newphase:
            # raced, but got correct result
            return 1
        else:
            return 0
    finally:
        lock.release()

def visibleheads(repo):
    """return the set of visible head of this repo"""
    # XXX we want a cache on this
    sroots = repo._phaseroots[secret]
    if sroots:
        # XXX very slow revset. storing heads or secret "boundary" would help.
        revset = repo.set('heads(not (%ln::))', sroots)

        vheads = [ctx.node() for ctx in revset]
        if not vheads:
            vheads.append(nullid)
    else:
        vheads = repo.heads()
    return vheads

def visiblebranchmap(repo):
    """return a branchmap for the visible set"""
    # XXX Recomputing this data on the fly is very slow.  We should build a
    # XXX cached version while computin the standard branchmap version.
    sroots = repo._phaseroots[secret]
    if sroots:
        vbranchmap = {}
        for branch, nodes in  repo.branchmap().iteritems():
            # search for secret heads.
            for n in nodes:
                if repo[n].phase() >= secret:
                    nodes = None
                    break
            # if secreat heads where found we must compute them again
            if nodes is None:
                s = repo.set('heads(branch(%s) - secret())', branch)
                nodes = [c.node() for c in s]
            vbranchmap[branch] = nodes
    else:
        vbranchmap = repo.branchmap()
    return vbranchmap

def analyzeremotephases(repo, subset, roots):
    """Compute phases heads and root in a subset of node from root dict

    * subset is heads of the subset
    * roots is {<nodeid> => phase} mapping. key and value are string.

    Accept unknown element input
    """
    # build list from dictionary
    draftroots = []
    nodemap = repo.changelog.nodemap # to filter unknown nodes
    for nhex, phase in roots.iteritems():
        if nhex == 'publishing': # ignore data related to publish option
            continue
        node = bin(nhex)
        phase = int(phase)
        if phase == 0:
            if node != nullid:
                repo.ui.warn(_('ignoring inconsistent public root'
                               ' from remote: %s\n') % nhex)
        elif phase == 1:
            if node in nodemap:
                draftroots.append(node)
        else:
            repo.ui.warn(_('ignoring unexpected root from remote: %i %s\n')
                         % (phase, nhex))
    # compute heads
    publicheads = newheads(repo, subset, draftroots)
    return publicheads, draftroots

def newheads(repo, heads, roots):
    """compute new head of a subset minus another

    * `heads`: define the first subset
    * `rroots`: define the second we substract to the first"""
    revset = repo.set('heads((%ln + parents(%ln)) - (%ln::%ln))',
                      heads, roots, roots, heads)
    return [c.node() for c in revset]


def newcommitphase(ui):
    """helper to get the target phase of new commit

    Handle all possible values for the phases.new-commit options.

    """
    v = ui.config('phases', 'new-commit', draft)
    try:
        return phasenames.index(v)
    except ValueError:
        try:
            return int(v)
        except ValueError:
            msg = _("phases.new-commit: not a valid phase name ('%s')")
            raise error.ConfigError(msg % v)

