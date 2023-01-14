# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Copyright Olivia Mackall <olivia@selenic.com> and others

""" Mercurial phases support code

    ---

    Copyright 2011 Pierre-Yves David <pierre-yves.david@ens-lyon.org>
                   Logilab SA        <contact@logilab.fr>
                   Augie Fackler     <durin42@gmail.com>

    This software may be used and distributed according to the terms
    of the GNU General Public License version 2 or any later version.

    ---

This module implements most phase logic in mercurial.

Basic Concept
=============

A 'changeset phase' is an indicator that tells us how a changeset is
manipulated and communicated. The details of each phase is described
below, here we describe the properties they have in common.

Like bookmarks, phases are not stored in history and thus are not
permanent and leave no audit trail.

First, no changeset can be in two phases at once. Phases are ordered,
so they can be considered from lowest to highest. The default, lowest
phase is 'public' - this is the normal phase of existing changesets. A
child changeset can not be in a lower phase than its parents.

These phases share a hierarchy of traits:

            immutable shared
    public:     X        X
    draft:               X
    secret:

Local commits are draft by default.

Phase Movement and Exchange
===========================

Phase data is exchanged by pushkey on pull and push. Some servers have
a publish option set, we call such a server a "publishing server".
Pushing a draft changeset to a publishing server changes the phase to
public.

A small list of fact/rules define the exchange of phase:

* old client never changes server states
* pull never changes server states
* publish and old server changesets are seen as public by client
* any secret changeset seen in another repository is lowered to at
  least draft

Here is the final table summing up the 49 possible use cases of phase
exchange:

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
    * X = not tracked (i.e., the old client or server has no internal
          way of recording the phase.)

    passive = only pushes

    A cell here can be read like this:

    "When a new client pushes a draft changeset (D) to a publishing
    server where it's not present (N), it's marked public on both
    sides (P/P)."

Note: old client behave as a publishing server with draft only content
- other people see it as public
- content is pushed as draft

"""

from __future__ import absolute_import

import errno
import struct
from typing import Dict

import bindings
from edenscm.util import sortdict

from . import error, perftrace, pycompat, smartset, txnutil, util, visibility
from .i18n import _
from .node import bbin, bin, hex, nullid, nullrev, short
from .pycompat import encodeutf8, range


_fphasesentry = struct.Struct(">i20s")

allphases = public, draft, secret = list(range(3))
trackedphases = allphases[1:]
phasenames = ["public", "draft", "secret"]


def _readroots(repo, phasedefaults=None):
    """Read phase roots from disk

    phasedefaults is a list of fn(repo, roots) callable, which are
    executed if the phase roots file does not exist. When phases are
    being initialized on an existing repository, this could be used to
    set selected changesets phase to something else than public.

    Return (roots, dirty) where dirty is true if roots differ from
    what is being stored.
    """
    dirty = False
    roots = [set() for i in allphases]
    try:
        f, pending = txnutil.trysharedpending(
            repo.root, repo.sharedroot, repo.svfs, "phaseroots"
        )
        try:
            for line in f:
                phase, nh = line.split()
                roots[int(phase)].add(bbin(nh))
        finally:
            f.close()
    except IOError as inst:
        if inst.errno != errno.ENOENT:
            raise
        if phasedefaults:
            for f in phasedefaults:
                roots = f(repo, roots)
        dirty = True
    return roots, dirty


def binaryencode(phasemapping) -> bytes:
    """encode a 'phase -> nodes' mapping into a binary stream

    Since phases are integer the mapping is actually a python list:
    [[PUBLIC_HEADS], [DRAFTS_HEADS], [SECRET_HEADS]]
    """
    binarydata = []
    for phase, nodes in enumerate(phasemapping):
        for head in nodes:
            binarydata.append(_fphasesentry.pack(phase, head))
    return b"".join(binarydata)


def binarydecode(stream):
    """decode a binary stream into a 'phase -> nodes' mapping

    Since phases are integer the mapping is actually a python list."""
    headsbyphase = [[] for i in allphases]
    entrysize = _fphasesentry.size
    while True:
        entry = stream.read(entrysize)
        if len(entry) < entrysize:
            if entry:
                raise error.Abort(_("bad phase-heads stream"))
            break
        phase, node = _fphasesentry.unpack(entry)
        headsbyphase[phase].append(node)
    return headsbyphase


def _trackphasechange(data, rev, old, new) -> None:
    """add a phase move the <data> dictionnary

    If data is None, nothing happens.
    """
    if data is None:
        return
    existing = data.get(rev)
    if existing is not None:
        old = existing[0]
    data[rev] = (old, new)


class phasecache(object):
    def __init__(self, repo, phasedefaults, _load=True):
        self._headbased = repo and repo.ui.configbool("experimental", "narrow-heads")
        if self._headbased:
            self._draftrevs = None
            self._publicrevs = None
            if _load:
                self.draftrevs(repo)
                self.publicrevs(repo)
        else:
            if _load:
                # Cheap trick to allow shallow-copy without copy module
                self.phaseroots, self.dirty = _readroots(repo, phasedefaults)
                self._loadedrevslen = 0
                self._phasesets = None
                self.filterunknown(repo)
                self.opener = repo.svfs

    def getrevset(self, repo, phases, subset=None):
        """return a smartset for the given phases"""
        if self._headbased:
            revs = None
            if public in phases:
                revs = self.publicrevs(repo)
            if draft in phases:
                if revs is None:
                    revs = self.draftrevs(repo)
                else:
                    # Ideally we'd just add draftrevs and publicrevs together.
                    # Unfortunately the add implementation currently sorts
                    # the result, causing it to no longer be lazy. Let's use
                    # addset instead, which lazily sorts.
                    revs = smartset.addset(revs, self.draftrevs(repo))

            if revs is None:
                revs = smartset.idset(bindings.dag.spans([]), repo=repo)

            # XXX: 'secret' is treated as an ampty set.
            if subset is not None:
                revs = subset & revs
            return revs

        self.loadphaserevs(repo)  # ensure phase's sets are loaded
        phases = set(phases)
        if public not in phases:
            # fast path: _phasesets contains the interesting sets,
            # might only need a union and post-filtering.
            if len(phases) == 1:
                [p] = phases
                revs = self._phasesets[p]
            else:
                revs = set.union(*[self._phasesets[p] for p in phases])
            if subset is None:
                return smartset.baseset(revs, repo=repo)
            else:
                return subset & smartset.baseset(revs, repo=repo)
        else:
            phases = set(allphases).difference(phases)
            if not phases:
                return smartset.fullreposet(repo)
            if len(phases) == 1:
                [p] = phases
                revs = self._phasesets[p]
            else:
                revs = set.union(*[self._phasesets[p] for p in phases])
            if subset is None:
                subset = smartset.fullreposet(repo)
            if not revs:
                return subset
            return subset.filter(lambda r: r not in revs)

    def copy(self):
        if self._headbased:
            return self
        # Shallow copy meant to ensure isolation in
        # advance/retractboundary(), nothing more.
        ph = self.__class__(None, None, _load=False)
        ph.phaseroots = self.phaseroots[:]
        ph.dirty = self.dirty
        ph.opener = self.opener
        ph._loadedrevslen = self._loadedrevslen
        ph._phasesets = self._phasesets
        return ph

    def replace(self, phcache):
        """replace all values in 'self' with content of phcache"""
        if self._headbased:
            return
        for a in ("phaseroots", "dirty", "opener", "_loadedrevslen", "_phasesets"):
            setattr(self, a, getattr(phcache, a))

    def _getphaserevsnative(self, repo):
        assert not self._headbased
        nativeroots = []
        for phase in trackedphases:
            nativeroots.append(list(map(repo.changelog.rev, self.phaseroots[phase])))
        return repo.changelog.computephases(nativeroots)

    def _computephaserevspure(self, repo):
        assert not self._headbased
        cl = repo.changelog
        self._phasesets = [set() for phase in allphases]
        roots = pycompat.maplist(cl.rev, self.phaseroots[secret])
        if roots:
            ps = set(cl.descendants(roots))
            for root in roots:
                ps.add(root)
            self._phasesets[secret] = ps
        roots = pycompat.maplist(cl.rev, self.phaseroots[draft])
        if roots:
            ps = set(cl.descendants(roots))
            for root in roots:
                ps.add(root)
            ps.difference_update(self._phasesets[secret])
            self._phasesets[draft] = ps
        self._loadedrevslen = len(cl)

    def draftrevs(self, repo):
        if not self._headbased:
            raise error.ProgrammingError(
                "non-headbased phases should use loadphaserevs"
            )

        if self._draftrevs is None:
            self.publicrevs(repo)

        return self._draftrevs

    def publicrevs(self, repo):
        if not self._headbased:
            raise error.ProgrammingError(
                "non-headbased phases should use loadphaserevs"
            )

        if self._publicrevs is None:
            cl = repo.changelog
            publicheadnodes = repo.heads(includepublic=True, includedraft=False)
            draftheadnodes = repo.heads(includepublic=False, includedraft=True)
            draftnodes, publicnodes = cl.dag.onlyboth(draftheadnodes, publicheadnodes)
            self._publicrevs = cl.torevset(publicnodes)
            self._draftrevs = cl.torevset(draftnodes)
        return self._publicrevs

    def loadphaserevs(self, repo):
        """ensure phase information is loaded in the object"""
        if self._headbased:
            raise error.ProgrammingError(
                "headbased phases should use loaddraftpublicrevs"
            )

        if self._phasesets is None:
            try:
                res = self._getphaserevsnative(repo)
                self._loadedrevslen, self._phasesets = res
            except AttributeError:
                self._computephaserevspure(repo)

    def invalidate(self):
        if self._headbased:
            self._draftrevs = None
            self._publicrevs = None
            return
        self._loadedrevslen = 0
        self._phasesets = None

    def phase(self, repo, rev):
        # We need a repo argument here to be able to build _phasesets
        # if necessary. The repository instance is not stored in
        # phasecache to avoid reference cycles. The changelog instance
        # is not stored because it is a filecache() property and can
        # be replaced without us being notified.
        if rev == nullrev:
            return public
        if rev < nullrev:
            raise ValueError(_("cannot lookup negative revision"))
        if self._headbased:
            if rev in self.draftrevs(repo):
                return draft
            if rev in self.publicrevs(repo):
                return public
            else:
                # 'secret' is repurposed as 'normally invisible' commits.
                return secret
        if rev >= self._loadedrevslen:
            self.invalidate()
            self.loadphaserevs(repo)
        for phase in trackedphases:
            if rev in self._phasesets[phase]:
                return phase
        return public

    def write(self):
        if self._headbased:
            # head-based phases are tracked by visible heads, and remotenames
            return
        if not self.dirty:
            return
        f = self.opener("phaseroots", "w", atomictemp=True, checkambig=True)
        try:
            self._write(f)
        finally:
            f.close()

    def _write(self, fp):
        assert not self._headbased
        for phase, roots in enumerate(self.phaseroots):
            for h in roots:
                fp.write(encodeutf8("%i %s\n" % (phase, hex(h))))
        self.dirty = False

    def _updateroots(self, phase, newroots, tr):
        assert not self._headbased
        self.phaseroots[phase] = newroots
        self.invalidate()
        self.dirty = True

        tr.addfilegenerator("phase", ("phaseroots",), self._write)
        tr.hookargs["phases_moved"] = "1"

    def registernew(self, repo, tr, targetphase, nodes):
        if self._headbased:
            # head-based phases do not need to track phase of new commits
            # explcitly. visible heads and remotenames track them implicitly.
            return
        self._retractboundary(repo, tr, targetphase, nodes)
        if tr is not None and "phases" in tr.changes:
            phasetracking = tr.changes["phases"]
            torev = repo.changelog.rev
            phase = self.phase
            for n in nodes:
                rev = torev(n)
                revphase = phase(repo, rev)
                _trackphasechange(phasetracking, rev, None, revphase)
        repo.invalidatevolatilesets()

    def advanceboundary(self, repo, tr, targetphase, nodes):
        """Set all 'nodes' to phase 'targetphase'

        Nodes with a phase lower than 'targetphase' are not affected.
        """
        if self._headbased:
            # head-based phases do not support editing phases directly
            return
        # Be careful to preserve shallow-copied values: do not update
        # phaseroots values, replace them.
        if tr is None:
            phasetracking = None
        else:
            phasetracking = tr.changes.get("phases")

        delroots = []  # set of root deleted by this path
        for phase in range(targetphase + 1, len(allphases)):
            # filter nodes that are not in a compatible phase already
            nodes = [n for n in nodes if self.phase(repo, repo[n].rev()) >= phase]
            if not nodes:
                break  # no roots to move anymore

            olds = self.phaseroots[phase]

            affected = repo.revs("%ln::%ln", olds, nodes)
            for r in affected:
                _trackphasechange(phasetracking, r, self.phase(repo, r), targetphase)

            roots = set(
                ctx.node() for ctx in repo.set("roots((%ln::) - %ld)", olds, affected)
            )
            if olds != roots:
                self._updateroots(phase, roots, tr)
                if targetphase == public:
                    visibility.phaseadjust(
                        repo, tr, newpublic=set(repo[r].node() for r in affected)
                    )
                # some roots may need to be declared for lower phases
                delroots.extend(olds - roots)
        # declare deleted root in the target phase
        if targetphase != 0:
            self._retractboundary(repo, tr, targetphase, delroots)
        repo.invalidatevolatilesets()

    def retractboundary(self, repo, tr, targetphase, nodes):
        if self._headbased:
            # head-based phases do not support editing phases directly
            return
        oldroots = self.phaseroots[: targetphase + 1]
        if tr is None:
            phasetracking = None
        else:
            phasetracking = tr.changes.get("phases")
        if (
            self._retractboundary(repo, tr, targetphase, nodes)
            and phasetracking is not None
        ):

            # find the affected revisions
            new = self.phaseroots[targetphase]
            old = oldroots[targetphase]
            affected = set(repo.revs("(%ln::) - (%ln::)", new, old))

            # find the phase of the affected revision
            for phase in range(targetphase, -1, -1):
                if phase:
                    roots = oldroots[phase]
                    revs = set(repo.revs("%ln::%ld", roots, affected))
                    affected -= revs
                else:  # public phase
                    revs = affected
                for r in revs:
                    _trackphasechange(phasetracking, r, phase, targetphase)
            visibility.phaseadjust(
                repo, tr, newdraft=set(repo[r].node() for r in affected)
            )
        repo.invalidatevolatilesets()

    def _retractboundary(self, repo, tr, targetphase, nodes):
        assert not self._headbased
        # Be careful to preserve shallow-copied values: do not update
        # phaseroots values, replace them.
        currentroots = self.phaseroots[targetphase]
        finalroots = oldroots = set(currentroots)
        newroots = [n for n in nodes if self.phase(repo, repo[n].rev()) < targetphase]
        if newroots:

            if nullid in newroots:
                raise error.Abort(_("cannot change null revision phase"))
            currentroots = currentroots.copy()
            currentroots.update(newroots)

            # Only compute new roots for revs above the roots that are being
            # retracted.
            minnewroot = min(repo[n].rev() for n in newroots)
            aboveroots = [n for n in currentroots if repo[n].rev() >= minnewroot]
            updatedroots = repo.set("roots(%ln::)", aboveroots)

            finalroots = set(n for n in currentroots if repo[n].rev() < minnewroot)
            finalroots.update(ctx.node() for ctx in updatedroots)
        if finalroots != oldroots:
            self._updateroots(targetphase, finalroots, tr)
            return True
        return False

    def filterunknown(self, repo):
        """remove unknown nodes from the phase boundary

        Nothing is lost as unknown nodes only hold data for their descendants.
        """
        if self._headbased:
            return
        filtered = False
        nodemap = repo.changelog.nodemap  # to filter unknown nodes
        for phase, nodes in enumerate(self.phaseroots):
            missing = sorted(node for node in nodes if node not in nodemap)
            if missing:
                for mnode in missing:
                    repo.ui.debug(
                        "removing unknown node %s from %i-phase boundary\n"
                        % (short(mnode), phase)
                    )
                nodes.symmetric_difference_update(missing)
                filtered = True
        if filtered:
            self.dirty = True
        # filterunknown is called by repo.destroyed, we may have no changes in
        # root but _phasesets contents is certainly invalid (or at least we
        # have not proper way to check that). related to issue 3858.
        #
        # The other caller is __init__ that have no _phasesets initialized
        # anyway. If this change we should consider adding a dedicated
        # "destroyed" function to phasecache or a proper cache key mechanism
        # (see branchmap one)
        self.invalidate()


def advanceboundary(repo, tr, targetphase, nodes) -> None:
    """Add nodes to a phase changing other nodes phases if necessary.

    This function move boundary *forward* this means that all nodes
    are set in the target phase or kept in a *lower* phase.

    Simplify boundary to contains phase roots only."""
    phcache = repo._phasecache.copy()
    phcache.advanceboundary(repo, tr, targetphase, nodes)
    repo._phasecache.replace(phcache)


def retractboundary(repo, tr, targetphase, nodes) -> None:
    """Set nodes back to a phase changing other nodes phases if
    necessary.

    This function move boundary *backward* this means that all nodes
    are set in the target phase or kept in a *higher* phase.

    Simplify boundary to contains phase roots only."""
    phcache = repo._phasecache.copy()
    phcache.retractboundary(repo, tr, targetphase, nodes)
    repo._phasecache.replace(phcache)


def registernew(repo, tr, targetphase, nodes) -> None:
    """register a new revision and its phase

    Code adding revisions to the repository should use this function to
    set new changeset in their target phase (or higher).
    """
    phcache = repo._phasecache.copy()
    phcache.registernew(repo, tr, targetphase, nodes)
    repo._phasecache.replace(phcache)


def listphases(repo) -> sortdict:
    """List phases root for serialization over pushkey"""
    # Use ordered dictionary so behavior is deterministic.
    keys = util.sortdict()
    if not repo._phasecache._headbased:
        value = "%i" % draft
        cl = repo.changelog
        for root in repo._phasecache.phaseroots[draft]:
            if repo._phasecache.phase(repo, cl.rev(root)) <= draft:
                keys[hex(root)] = value

    if repo.publishing():
        # Add an extra data to let remote know we are a publishing
        # repo. Publishing repo can't just pretend they are old repo.
        # When pushing to a publishing repo, the client still need to
        # push phase boundary
        #
        # Push do not only push changeset. It also push phase data.
        # New phase data may apply to common changeset which won't be
        # push (as they are common). Here is a very simple example:
        #
        # 1) repo A push changeset X as draft to repo B
        # 2) repo B make changeset X public
        # 3) repo B push to repo A. X is not pushed but the data that
        #    X as now public should
        #
        # The server can't handle it on it's own as it has no idea of
        # client phase data.
        keys["publishing"] = "True"
    return keys


def pushphase(repo, nhex, oldphasestr, newphasestr) -> bool:
    """List phases root for serialization over pushkey"""
    with repo.lock():
        currentphase = repo[nhex].phase()
        newphase = abs(int(newphasestr))  # let's avoid negative index surprise
        oldphase = abs(int(oldphasestr))  # let's avoid negative index surprise
        if currentphase == oldphase and newphase < oldphase:
            with repo.transaction("pushkey-phase") as tr:
                advanceboundary(repo, tr, newphase, [bin(nhex)])
            return True
        elif currentphase == newphase:
            # raced, but got correct result
            return True
        else:
            return False


def subsetphaseheads(repo, subset):
    """Finds the phase heads for a subset of a history

    Returns a list indexed by phase number where each item is a list of phase
    head nodes.
    """
    cl = repo.changelog

    headsbyphase = [[] for i in allphases]
    # No need to keep track of secret phase; any heads in the subset that
    # are not mentioned are implicitly secret.
    for phase in allphases[:-1]:
        revset = "heads(%%ln & %s())" % phasenames[phase]
        headsbyphase[phase] = [cl.node(r) for r in repo.revs(revset, subset)]
    return headsbyphase


@perftrace.tracefunc("Update Phases")
def updatephases(repo, trgetter, headsbyphase) -> None:
    """Updates the repo with the given phase heads"""
    # Head-bases phases do not need update.
    if repo._phasecache._headbased:
        return

    # Now advance phase boundaries of all but secret phase
    #
    # run the update (and fetch transaction) only if there are actually things
    # to update. This avoid creating empty transaction during no-op operation.

    for phase in allphases[:-1]:
        revset = "%%ln - %s()" % phasenames[phase]
        heads = [c.node() for c in repo.set(revset, headsbyphase[phase])]
        if heads:
            advanceboundary(repo, trgetter(), phase, heads)


def analyzeremotephases(repo, subset, roots):
    """Compute phases heads and root in a subset of node from root dict

    * subset is heads of the subset
    * roots is {<nodeid> => phase} mapping. key and value are string.

    Accept unknown element input
    """
    # build list from dictionary
    draftroots = []
    nodemap = repo.changelog.nodemap  # to filter unknown nodes
    for nhex, phase in pycompat.iteritems(roots):
        if nhex == "publishing":  # ignore data related to publish option
            continue
        node = bin(nhex)
        phase = int(phase)
        if phase == public:
            if node != nullid:
                repo.ui.warn(
                    _("ignoring inconsistent public root" " from remote: %s\n") % nhex
                )
        elif phase == draft:
            if node in nodemap:
                draftroots.append(node)
        else:
            repo.ui.warn(
                _("ignoring unexpected root from remote: %i %s\n") % (phase, nhex)
            )
    # compute heads
    publicheads = newheads(repo, subset, draftroots)
    return publicheads, draftroots


class remotephasessummary(object):
    """summarize phase information on the remote side

    :publishing: True is the remote is publishing
    :publicheads: list of remote public phase heads (nodes)
    :draftheads: list of remote draft phase heads (nodes)
    :draftroots: list of remote draft phase root (nodes)
    """

    def __init__(self, repo, remotesubset, remoteroots):
        self._allremoteroots = remoteroots

        self.publishing = remoteroots.get("publishing", False)

        ana = analyzeremotephases(repo, remotesubset, remoteroots)
        self.publicheads, self.draftroots = ana
        # Get the list of all "heads" revs draft on remote
        dheads = repo.set("heads(%ln::%ln)", self.draftroots, remotesubset)
        self.draftheads = [c.node() for c in dheads]


def newheads(repo, heads, roots):
    """compute new head of a subset minus another

    * `heads`: define the first subset
    * `roots`: define the second we subtract from the first"""
    revset = repo.set(
        "heads((%ln + parents(%ln)) - (%ln::%ln))", heads, roots, roots, heads
    )
    return [c.node() for c in revset]


def newcommitphase(ui) -> int:
    """helper to get the target phase of new commit

    Handle all possible values for the phases.new-commit options.

    """
    v = ui.config("phases", "new-commit")
    try:
        return phasenames.index(v)
    except ValueError:
        try:
            return int(v)
        except ValueError:
            msg = _("phases.new-commit: not a valid phase name ('%s')")
            raise error.ConfigError(msg % v)


def hassecret(repo) -> bool:
    """utility function that check if a repo have any secret changeset."""
    if repo.ui.configbool("experimental", "narrow-heads"):
        return False
    return bool(repo._phasecache.phaseroots[2])


def preparehookargs(node, old: str, new) -> Dict[str, str]:
    if old is None:
        old = ""
    else:
        # pyre-fixme[6]: For 1st param expected `SupportsIndex` but got `str`.
        old = phasenames[old]
    return {"node": node, "oldphase": old, "phase": phasenames[new]}
