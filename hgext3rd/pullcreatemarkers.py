# pullcreatemarkers.py - create obsolescence markers on pull for better rebases
#
# Copyright 2015 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
#
# The goal of this extensions is to create obsolescence markers locally for
# commits previously landed.
# It uses the phabricator revision number in the commit message to detect the
# relationship between a draft commit and its landed counterpart.
# Thanks to these markers, less information is displayed and rebases can have
# less irrelevant conflicts.

from mercurial import commands
from mercurial import obsolete
from mercurial import phases
from mercurial import extensions
from phabricator import diffprops

def getdiff(rev):
    phabrev = diffprops.parserevfromcommitmsg(rev.description())
    return int(phabrev) if phabrev else None

def extsetup(ui):
    extensions.wrapcommand(commands.table, 'pull', _pull)

def _pull(orig, ui, repo, *args, **opts):
    if not obsolete.isenabled(repo, obsolete.createmarkersopt):
        return orig(ui, repo, *args, **opts)
    maxrevbeforepull = len(repo.changelog)
    r = orig(ui, repo, *args, **opts)
    maxrevafterpull = len(repo.changelog)

    # Collect the diff number of the landed diffs
    landeddiffs = {}
    for rev in range(maxrevbeforepull, maxrevafterpull):
        n = repo[rev]
        if n.phase() == phases.public:
            diff = getdiff(n)
            if diff is not None:
                landeddiffs[diff] = n

    if not landeddiffs:
        return r

    # Try to find match with the drafts
    tocreate = []
    unfiltered = repo.unfiltered()
    for rev in unfiltered.revs("draft() - hidden()"):
        n = unfiltered[rev]
        diff = getdiff(n)
        if diff in landeddiffs:
            tocreate.append((n, (landeddiffs[diff],)))

    if not tocreate:
        return r

    inhibit, deinhibitnodes = _deinhibitancestors(unfiltered, tocreate)

    with unfiltered.lock():
        with unfiltered.transaction('pullcreatemarkers'):
            obsolete.createmarkers(unfiltered, tocreate)
            if deinhibitnodes:
                inhibit._deinhibitmarkers(unfiltered, deinhibitnodes)

    return r

def _deinhibitancestors(repo, markers):
    """Compute the set of commits that already have obsolescence markers
    which were possibly inhibited, and should be deinhibited because of this
    new pull operation.

    Returns a tuple of (inhibit module, node set).
    Returns (None, None) if the inhibit extension is not enabled."""
    try:
        inhibit = extensions.find('inhibit')
    except KeyError:
        return None, None

    if not inhibit._inhibitenabled(repo):
        return None, None

    # Commits for which we should deinhibit obsolescence markers
    deinhibitset = set()
    # Commits whose parents we should process
    toprocess = set([ctx for ctx, successor in markers])
    # Commits that are already in toprocess or have already been processed
    seen = toprocess.copy()
    # Commits that we deinhibit obsolescence markers for
    while toprocess:
        ctx = toprocess.pop()
        for p in ctx.parents():
            if p in seen:
                continue
            seen.add(p)
            if _isobsolete(p):
                deinhibitset.add(p.node())
                toprocess.add(p)

    return inhibit, deinhibitset

def _isobsolete(ctx):
    # A commit is obsolete if it has at least one successor marker.
    #
    # successormarkers() returns a generator.  generators unfortunately
    # evaluate to True even if they are "empty", so pull one element off and
    # see if anything exists or not.
    i = obsolete.successormarkers(ctx)
    try:
        next(i)
    except StopIteration:
        return False
    return True
