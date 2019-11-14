# Portions Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# obsutil.py - utility functions for obsolescence
#
# Copyright 2017 Boris Feld <boris.feld@octobus.net>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from . import phases, util


class marker(object):
    """Wrap obsolete marker raw data"""

    def __init__(self, repo, data):
        # the repo argument will be used to create changectx in later version
        self._repo = repo
        self._data = data
        self._decodedmeta = None

    def __hash__(self):
        return hash(self._data)

    def __eq__(self, other):
        if type(other) != type(self):
            return False
        return self._data == other._data

    def precnode(self):
        msg = "'marker.precnode' is deprecated, " "use 'marker.prednode'"
        util.nouideprecwarn(msg, "4.4")
        return self.prednode()

    def prednode(self):
        """Predecessor changeset node identifier"""
        return self._data[0]

    def succnodes(self):
        """List of successor changesets node identifiers"""
        return self._data[1]

    def parentnodes(self):
        """Parents of the predecessors (None if not recorded)"""
        return self._data[5]

    def metadata(self):
        """Decoded metadata dictionary"""
        return dict(self._data[3])

    def date(self):
        """Creation date as (unixtime, offset)"""
        return self._data[4]

    def flags(self):
        """The flags field of the marker"""
        return self._data[2]


def getmarkers(repo, nodes=None, exclusive=False):
    """returns markers known in a repository

    If <nodes> is specified, only markers "relevant" to those nodes are are
    returned"""
    if nodes is None:
        rawmarkers = repo.obsstore
    elif exclusive:
        rawmarkers = exclusivemarkers(repo, nodes)
    else:
        rawmarkers = repo.obsstore.relevantmarkers(nodes)

    for markerdata in rawmarkers:
        yield marker(repo, markerdata)


def closestpredecessors(repo, nodeid):
    """yield the list of next predecessors pointing on visible changectx nodes

    This function respect the repoview filtering, filtered revision will be
    considered missing.
    """

    precursors = repo.obsstore.predecessors
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


def allprecursors(*args, **kwargs):
    """ (DEPRECATED)
    """
    msg = "'obsutil.allprecursors' is deprecated, " "use 'obsutil.allpredecessors'"
    util.nouideprecwarn(msg, "4.4")

    return allpredecessors(*args, **kwargs)


def allpredecessors(obsstore, nodes, ignoreflags=0, startdepth=None, stopdepth=None):
    """Yield node for every precursors of <nodes>.

    Some precursors may be unknown locally.

    This is a linear yield unsuited to detecting folded changesets. It includes
    initial nodes too, if startdepth is None or 0."""
    depth = 0
    thislevel = set(nodes)
    nextlevel = set()
    seen = set(thislevel)
    while thislevel and (stopdepth is None or depth < stopdepth):
        for current in thislevel:
            if startdepth is None or depth >= startdepth:
                yield current
            for mark in obsstore.predecessors.get(current, ()):
                # ignore marker flagged with specified flag
                if mark[2] & ignoreflags:
                    continue
                nextnode = mark[0]
                if nextnode not in seen:
                    seen.add(nextnode)
                    nextlevel.add(nextnode)
        thislevel = nextlevel
        nextlevel = set()
        depth += 1


def allsuccessors(obsstore, nodes, ignoreflags=0, startdepth=None, stopdepth=None):
    """Yield node for every successor of <nodes>.

    Some successors may be unknown locally.

    This is a linear yield unsuited to detecting split changesets. It includes
    initial nodes too, if startdepth is None or 0."""
    depth = 0
    thislevel = set(nodes)
    nextlevel = set()
    seen = set(thislevel)
    while thislevel and (stopdepth is None or depth < stopdepth):
        for current in thislevel:
            if startdepth is None or depth >= startdepth:
                yield current
            for mark in obsstore.successors.get(current, ()):
                # ignore marker flagged with specified flag
                if mark[2] & ignoreflags:
                    continue
                for nextnode in mark[1]:
                    if nextnode not in seen:
                        seen.add(nextnode)
                        nextlevel.add(nextnode)
        thislevel = nextlevel
        nextlevel = set()
        depth += 1


def _filterprunes(markers):
    """return a set with no prune markers"""
    return set(m for m in markers if m[1])


def exclusivemarkers(repo, nodes):
    """set of markers relevant to "nodes" but no other locally-known nodes

    This function compute the set of markers "exclusive" to a locally-known
    node. This means we walk the markers starting from <nodes> until we reach a
    locally-known precursors outside of <nodes>. Element of <nodes> with
    locally-known successors outside of <nodes> are ignored (since their
    precursors markers are also relevant to these successors).

    For example:

        # (A0 rewritten as A1)
        #
        # A0 <-1- A1 # Marker "1" is exclusive to A1

        or

        # (A0 rewritten as AX; AX rewritten as A1; AX is unkown locally)
        #
        # <-1- A0 <-2- AX <-3- A1 # Marker "2,3" are exclusive to A1

        or

        # (A0 has unknown precursors, A0 rewritten as A1 and A2 (divergence))
        #
        #          <-2- A1 # Marker "2" is exclusive to A0,A1
        #        /
        # <-1- A0
        #        \
        #         <-3- A2 # Marker "3" is exclusive to A0,A2
        #
        # in addition:
        #
        #  Markers "2,3" are exclusive to A1,A2
        #  Markers "1,2,3" are exclusive to A0,A1,A2

        See test/test-obsolete-bundle-strip.t for more examples.

    An example usage is strip. When stripping a changeset, we also want to
    strip the markers exclusive to this changeset. Otherwise we would have
    "dangling"" obsolescence markers from its precursors: Obsolescence markers
    marking a node as obsolete without any successors available locally.

    As for relevant markers, the prune markers for children will be followed.
    Of course, they will only be followed if the pruned children is
    locally-known. Since the prune markers are relevant to the pruned node.
    However, while prune markers are considered relevant to the parent of the
    pruned changesets, prune markers for locally-known changeset (with no
    successors) are considered exclusive to the pruned nodes. This allows
    to strip the prune markers (with the rest of the exclusive chain) alongside
    the pruned changesets.
    """
    # running on a filtered repository would be dangerous as markers could be
    # reported as exclusive when they are relevant for other filtered nodes.
    unfi = repo.unfiltered()

    # shortcut to various useful item
    nm = unfi.changelog.nodemap
    precursorsmarkers = unfi.obsstore.predecessors
    successormarkers = unfi.obsstore.successors
    childrenmarkers = unfi.obsstore.children

    # exclusive markers (return of the function)
    exclmarkers = set()
    # we need fast membership testing
    nodes = set(nodes)
    # looking for head in the obshistory
    #
    # XXX we are ignoring all issues in regard with cycle for now.
    stack = [n for n in nodes if not _filterprunes(successormarkers.get(n, ()))]
    stack.sort()
    # nodes already stacked
    seennodes = set(stack)
    while stack:
        current = stack.pop()
        # fetch precursors markers
        markers = list(precursorsmarkers.get(current, ()))
        # extend the list with prune markers
        for mark in successormarkers.get(current, ()):
            if not mark[1]:
                markers.append(mark)
        # and markers from children (looking for prune)
        for mark in childrenmarkers.get(current, ()):
            if not mark[1]:
                markers.append(mark)
        # traverse the markers
        for mark in markers:
            if mark in exclmarkers:
                # markers already selected
                continue

            # If the markers is about the current node, select it
            #
            # (this delay the addition of markers from children)
            if mark[1] or mark[0] == current:
                exclmarkers.add(mark)

            # should we keep traversing through the precursors?
            prec = mark[0]

            # nodes in the stack or already processed
            if prec in seennodes:
                continue

            # is this a locally known node ?
            known = prec in nm
            # if locally-known and not in the <nodes> set the traversal
            # stop here.
            if known and prec not in nodes:
                continue

            # do not keep going if there are unselected markers pointing to this
            # nodes. If we end up traversing these unselected markers later the
            # node will be taken care of at that point.
            precmarkers = _filterprunes(successormarkers.get(prec))
            if precmarkers.issubset(exclmarkers):
                seennodes.add(prec)
                stack.append(prec)

    return exclmarkers


def foreground(repo, nodes):
    """return all nodes in the "foreground" of other node

    The foreground of a revision is anything reachable using parent -> children
    or precursor -> successor relation. It is very similar to "descendant" but
    augmented with obsolescence information.

    Beware that possible obsolescence cycle may result if complex situation.
    """
    repo = repo.unfiltered()
    foreground = set(repo.set("%ln::", nodes))
    if repo.obsstore:
        # We only need this complicated logic if there is obsolescence
        # XXX will probably deserve an optimised revset.
        nm = repo.changelog.nodemap
        plen = -1
        # compute the whole set of successors or descendants
        while len(foreground) != plen:
            plen = len(foreground)
            succs = set(c.node() for c in foreground)
            mutable = [c.node() for c in foreground if c.mutable()]
            succs.update(allsuccessors(repo.obsstore, mutable))
            known = (n for n in succs if n in nm)
            foreground = set(repo.set("%ln::", known))
    return set(c.node() for c in foreground)


def getobsoleted(repo, tr):
    """return the set of pre-existing revisions obsoleted by a transaction"""
    torev = repo.unfiltered().changelog.nodemap.get
    phase = repo._phasecache.phase
    succsmarkers = repo.obsstore.successors.get
    public = phases.public
    addedmarkers = tr.changes.get("obsmarkers")
    addedrevs = tr.changes.get("revs")
    seenrevs = set()
    obsoleted = set()
    for mark in addedmarkers:
        node = mark[0]
        rev = torev(node)
        if rev is None or rev in seenrevs or rev in addedrevs:
            continue
        seenrevs.add(rev)
        if phase(repo, rev) == public:
            continue
        if set(succsmarkers(node) or []).issubset(addedmarkers):
            obsoleted.add(rev)
    return obsoleted


class _succs(list):
    """small class to represent a successors with some metadata about it"""

    def __init__(self, *args, **kwargs):
        super(_succs, self).__init__(*args, **kwargs)
        self.markers = set()

    def copy(self):
        new = _succs(self)
        new.markers = self.markers.copy()
        return new

    @util.propertycache
    def _set(self):
        # immutable
        return set(self)

    def canmerge(self, other):
        return self._set.issubset(other._set)


def successorssets(repo, initialnode, closest=False, cache=None):
    """Return set of all latest successors of initial nodes

    The successors set of a changeset A are the group of revisions that succeed
    A. It succeeds A as a consistent whole, each revision being only a partial
    replacement. By default, the successors set contains non-obsolete
    changesets only, walking the obsolescence graph until reaching a leaf. If
    'closest' is set to True, closest successors-sets are return (the
    obsolescence walk stops on known changesets).

    This function returns the full list of successor sets which is why it
    returns a list of tuples and not just a single tuple. Each tuple is a valid
    successors set. Note that (A,) may be a valid successors set for changeset A
    (see below).

    In most cases, a changeset A will have a single element (e.g. the changeset
    A is replaced by A') in its successors set. Though, it is also common for a
    changeset A to have no elements in its successor set (e.g. the changeset
    has been pruned). Therefore, the returned list of successors sets will be
    [(A',)] or [], respectively.

    When a changeset A is split into A' and B', however, it will result in a
    successors set containing more than a single element, i.e. [(A',B')].
    Divergent changesets will result in multiple successors sets, i.e. [(A',),
    (A'')].

    If a changeset A is not obsolete, then it will conceptually have no
    successors set. To distinguish this from a pruned changeset, the successor
    set will contain itself only, i.e. [(A,)].

    Finally, final successors unknown locally are considered to be pruned
    (pruned: obsoleted without any successors). (Final: successors not affected
    by markers).

    The 'closest' mode respect the repoview filtering. For example, without
    filter it will stop at the first locally known changeset, with 'visible'
    filter it will stop on visible changesets).

    The optional `cache` parameter is a dictionary that may contains
    precomputed successors sets. It is meant to reuse the computation of a
    previous call to `successorssets` when multiple calls are made at the same
    time. The cache dictionary is updated in place. The caller is responsible
    for its life span. Code that makes multiple calls to `successorssets`
    *should* use this cache mechanism or risk a performance hit.

    Since results are different depending of the 'closest' most, the same cache
    cannot be reused for both mode.
    """

    succmarkers = repo.obsstore.successors

    # Stack of nodes we search successors sets for
    toproceed = [initialnode]
    # set version of above list for fast loop detection
    # element added to "toproceed" must be added here
    stackedset = set(toproceed)
    if cache is None:
        cache = {}

    # This while loop is the flattened version of a recursive search for
    # successors sets
    #
    # def successorssets(x):
    #    successors = directsuccessors(x)
    #    ss = [[]]
    #    for succ in directsuccessors(x):
    #        # product as in itertools cartesian product
    #        ss = product(ss, successorssets(succ))
    #    return ss
    #
    # But we can not use plain recursive calls here:
    # - that would blow the python call stack
    # - obsolescence markers may have cycles, we need to handle them.
    #
    # The `toproceed` list act as our call stack. Every node we search
    # successors set for are stacked there.
    #
    # The `stackedset` is set version of this stack used to check if a node is
    # already stacked. This check is used to detect cycles and prevent infinite
    # loop.
    #
    # successors set of all nodes are stored in the `cache` dictionary.
    #
    # After this while loop ends we use the cache to return the successors sets
    # for the node requested by the caller.
    while toproceed:
        # Every iteration tries to compute the successors sets of the topmost
        # node of the stack: CURRENT.
        #
        # There are four possible outcomes:
        #
        # 1) We already know the successors sets of CURRENT:
        #    -> mission accomplished, pop it from the stack.
        # 2) Stop the walk:
        #    default case: Node is not obsolete
        #    closest case: Node is known at this repo filter level
        #      -> the node is its own successors sets. Add it to the cache.
        # 3) We do not know successors set of direct successors of CURRENT:
        #    -> We add those successors to the stack.
        # 4) We know successors sets of all direct successors of CURRENT:
        #    -> We can compute CURRENT successors set and add it to the
        #       cache.
        #
        current = toproceed[-1]

        # case 2 condition is a bit hairy because of closest,
        # we compute it on its own
        case2condition = (current not in succmarkers) or (
            closest and current != initialnode and current in repo
        )

        if current in cache:
            # case (1): We already know the successors sets
            stackedset.remove(toproceed.pop())
        elif case2condition:
            # case (2): end of walk.
            if current in repo:
                # We have a valid successors.
                cache[current] = [_succs((current,))]
            else:
                # Final obsolete version is unknown locally.
                # Do not count that as a valid successors
                cache[current] = []
        else:
            # cases (3) and (4)
            #
            # We proceed in two phases. Phase 1 aims to distinguish case (3)
            # from case (4):
            #
            #     For each direct successors of CURRENT, we check whether its
            #     successors sets are known. If they are not, we stack the
            #     unknown node and proceed to the next iteration of the while
            #     loop. (case 3)
            #
            #     During this step, we may detect obsolescence cycles: a node
            #     with unknown successors sets but already in the call stack.
            #     In such a situation, we arbitrary set the successors sets of
            #     the node to nothing (node pruned) to break the cycle.
            #
            #     If no break was encountered we proceed to phase 2.
            #
            # Phase 2 computes successors sets of CURRENT (case 4); see details
            # in phase 2 itself.
            #
            # Note the two levels of iteration in each phase.
            # - The first one handles obsolescence markers using CURRENT as
            #   precursor (successors markers of CURRENT).
            #
            #   Having multiple entry here means divergence.
            #
            # - The second one handles successors defined in each marker.
            #
            #   Having none means pruned node, multiple successors means split,
            #   single successors are standard replacement.
            #
            for mark in sorted(succmarkers[current]):
                for suc in mark[1]:
                    if suc not in cache:
                        if suc in stackedset:
                            # cycle breaking
                            cache[suc] = []
                        else:
                            # case (3) If we have not computed successors sets
                            # of one of those successors we add it to the
                            # `toproceed` stack and stop all work for this
                            # iteration.
                            toproceed.append(suc)
                            stackedset.add(suc)
                            break
                else:
                    continue
                break
            else:
                # case (4): we know all successors sets of all direct
                # successors
                #
                # Successors set contributed by each marker depends on the
                # successors sets of all its "successors" node.
                #
                # Each different marker is a divergence in the obsolescence
                # history. It contributes successors sets distinct from other
                # markers.
                #
                # Within a marker, a successor may have divergent successors
                # sets. In such a case, the marker will contribute multiple
                # divergent successors sets. If multiple successors have
                # divergent successors sets, a Cartesian product is used.
                #
                # At the end we post-process successors sets to remove
                # duplicated entry and successors set that are strict subset of
                # another one.
                succssets = []
                for mark in sorted(succmarkers[current]):
                    # successors sets contributed by this marker
                    base = _succs()
                    base.markers.add(mark)
                    markss = [base]
                    for suc in mark[1]:
                        # cardinal product with previous successors
                        productresult = []
                        for prefix in markss:
                            for suffix in cache[suc]:
                                newss = prefix.copy()
                                newss.markers.update(suffix.markers)
                                for part in suffix:
                                    # do not duplicated entry in successors set
                                    # first entry wins.
                                    if part not in newss:
                                        newss.append(part)
                                productresult.append(newss)
                        markss = productresult
                    succssets.extend(markss)
                # remove duplicated and subset
                seen = []
                final = []
                candidates = sorted((s for s in succssets if s), key=len, reverse=True)
                for cand in candidates:
                    for seensuccs in seen:
                        if cand.canmerge(seensuccs):
                            seensuccs.markers.update(cand.markers)
                            break
                    else:
                        final.append(cand)
                        seen.append(cand)
                final.reverse()  # put small successors set first
                cache[current] = final
    return cache[initialnode]


def successorsandmarkers(repo, ctx):
    """compute the raw data needed for computing obsfate
    Returns a list of dict, one dict per successors set
    """
    if not ctx.obsolete():
        return None

    ssets = successorssets(repo, ctx.node(), closest=True)

    # closestsuccessors returns an empty list for pruned revisions, remap it
    # into a list containing an empty list for future processing
    if ssets == []:
        ssets = [[]]

    # Try to recover pruned markers
    succsmap = repo.obsstore.successors
    fullsuccessorsets = []  # successor set + markers
    for sset in ssets:
        if sset:
            fullsuccessorsets.append(sset)
        else:
            # successorsset return an empty set() when ctx or one of its
            # successors is pruned.
            # In this case, walk the obs-markers tree again starting with ctx
            # and find the relevant pruning obs-makers, the ones without
            # successors.
            # Having these markers allow us to compute some information about
            # its fate, like who pruned this changeset and when.

            # XXX we do not catch all prune markers (eg rewritten then pruned)
            # (fix me later)
            foundany = False
            for mark in succsmap.get(ctx.node(), ()):
                if not mark[1]:
                    foundany = True
                    sset = _succs()
                    sset.markers.add(mark)
                    fullsuccessorsets.append(sset)
            if not foundany:
                fullsuccessorsets.append(_succs())

    values = []
    for sset in fullsuccessorsets:
        values.append({"successors": sset, "markers": sset.markers})

    return values


def obsfateverb(successorset, markers):
    """ Return the verb summarizing the successorset and potentially using
    information from the markers
    """
    if not successorset:
        verb = "pruned"
    elif len(successorset) == 1:
        verb = "rewritten"
    else:
        verb = "split"
    return verb


def markersdates(markers):
    """returns the list of dates for a list of markers
    """
    return [m[4] for m in markers]


def markersusers(markers):
    """ Returns a sorted list of markers users without duplicates
    """
    markersmeta = [dict(m[3]) for m in markers]
    users = set(meta.get("user") for meta in markersmeta if meta.get("user"))

    return sorted(users)


def markersoperations(markers):
    """ Returns a sorted list of markers operations without duplicates
    """
    markersmeta = [dict(m[3]) for m in markers]
    operations = set(
        meta.get("operation") for meta in markersmeta if meta.get("operation")
    )

    return sorted(operations)


def obsfateprinter(successors, markers, ui):
    """ Build a obsfate string for a single successorset using all obsfate
    related function defined in obsutil
    """
    quiet = ui.quiet
    verbose = ui.verbose
    normal = not verbose and not quiet

    line = []

    # Verb
    line.append(obsfateverb(successors, markers))

    # Operations
    operations = markersoperations(markers)
    if operations:
        line.append(" using %s" % ", ".join(operations))

    # Successors
    if successors:
        fmtsuccessors = [successors.joinfmt(succ) for succ in successors]
        line.append(" as %s" % ", ".join(fmtsuccessors))

    # Users
    users = markersusers(markers)
    # Filter out current user in not verbose mode to reduce amount of
    # information
    if not verbose:
        currentuser = ui.username(acceptempty=True)
        if len(users) == 1 and currentuser in users:
            users = None

    if (verbose or normal) and users:
        line.append(" by %s" % ", ".join(users))

    # Date
    dates = markersdates(markers)

    if dates and verbose:
        min_date = min(dates)
        max_date = max(dates)

        if min_date == max_date:
            fmtmin_date = util.datestr(min_date, "%Y-%m-%d %H:%M %1%2")
            line.append(" (at %s)" % fmtmin_date)
        else:
            fmtmin_date = util.datestr(min_date, "%Y-%m-%d %H:%M %1%2")
            fmtmax_date = util.datestr(max_date, "%Y-%m-%d %H:%M %1%2")
            line.append(" (between %s and %s)" % (fmtmin_date, fmtmax_date))

    return "".join(line)
