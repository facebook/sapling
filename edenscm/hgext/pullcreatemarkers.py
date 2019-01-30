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
from edenscm.mercurial import commands, extensions, obsolete, phases

from .extlib.phabricator import diffprops
from .phabstatus import COMMITTEDSTATUS, getdiffstatus


def getdiff(rev):
    phabrev = diffprops.parserevfromcommitmsg(rev.description())
    return int(phabrev) if phabrev else None


def extsetup(ui):
    extensions.wrapcommand(commands.table, "pull", _pull)


def _pull(orig, ui, repo, *args, **opts):
    if not obsolete.isenabled(repo, obsolete.createmarkersopt):
        return orig(ui, repo, *args, **opts)

    maxrevbeforepull = len(repo.changelog)
    r = orig(ui, repo, *args, **opts)
    maxrevafterpull = len(repo.changelog)

    createmarkers(r, repo, maxrevbeforepull, maxrevafterpull)
    return r


def createmarkers(pullres, repo, start, stop, fromdrafts=True):
    landeddiffs = getlandeddiffs(repo, start, stop, onlypublic=fromdrafts)

    if not landeddiffs:
        return

    tocreate = (
        getmarkersfromdrafts(repo, landeddiffs)
        if fromdrafts
        else getmarkers(repo, landeddiffs)
    )

    if not tocreate:
        return

    unfiltered = repo.unfiltered()

    with unfiltered.lock(), unfiltered.transaction("pullcreatemarkers"):
        obsolete.createmarkers(unfiltered, tocreate)


def getlandeddiffs(repo, start, stop, onlypublic=True):
    landeddiffs = {}

    for rev in range(start, stop):
        rev = repo[rev]
        if not onlypublic or rev.phase() == phases.public:
            diff = getdiff(rev)
            if diff is not None:
                landeddiffs[diff] = rev
    return landeddiffs


def getmarkers(repo, landeddiffs):
    return [(landeddiffs[rev], tuple()) for rev in getlandedrevsiter(repo, landeddiffs)]


def getmarkersfromdrafts(repo, landeddiffs):
    tocreate = []
    unfiltered = repo.unfiltered()

    for rev in unfiltered.revs("draft() - obsolete()"):
        rev = unfiltered[rev]
        diff = getdiff(rev)

        if diff in landeddiffs and landeddiffs[diff].rev() != rev.rev():
            marker = (rev, (landeddiffs[diff],))
            tocreate.append(marker)
    return tocreate


def getlandedrevsiter(repo, landeddiffs):
    statuses = (
        status
        for status in getdiffstatus(repo, *landeddiffs.keys())
        if status != "Error"
    )

    return (
        diff
        for status, diff in zip(statuses, landeddiffs.keys())
        if status["status"] == COMMITTEDSTATUS
    )
