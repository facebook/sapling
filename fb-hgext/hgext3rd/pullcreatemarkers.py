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
    for rev in unfiltered.revs("draft() - obsolete()"):
        n = unfiltered[rev]
        diff = getdiff(n)
        if diff in landeddiffs and landeddiffs[diff].rev() != n.rev():
            tocreate.append((n, (landeddiffs[diff],)))

    if not tocreate:
        return r

    with unfiltered.lock(), unfiltered.transaction('pullcreatemarkers'):
        obsolete.createmarkers(unfiltered, tocreate)

    return r
