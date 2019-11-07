# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the

"""
mark commits as "Landed" on pull

Config::

    [pullcreatemarkers]
    # Use graphql to query what diffs are landed, instead of scanning
    # through pulled commits.
    use-graphql = true
"""
from ..mercurial import (
    commands,
    extensions,
    mutation,
    obsolete,
    phases,
    registrar,
    visibility,
)
from ..mercurial.i18n import _
from ..mercurial.node import short
from .extlib.phabricator import diffprops, graphql
from .phabstatus import COMMITTEDSTATUS, getdiffstatus


cmdtable = {}
command = registrar.command(cmdtable)


def _cleanuplanded(repo, dryrun=False, skipnodes=None):
    """Query Phabricator about states of draft commits and optionally mark them
    as landed.

    This uses mutation and visibility directly.
    """
    if skipnodes is None:
        skipnodes = set()
    difftodraft = {}  # {str: node}
    for ctx in repo.set("draft() - obsolete()"):
        if ctx.node() in skipnodes:
            continue
        diffid = diffprops.parserevfromcommitmsg(ctx.description())  # str or None
        if diffid:
            difftodraft.setdefault(diffid, []).append(ctx.node())

    client = graphql.Client(repo=repo)
    difftopublic = client.getlandednodes(list(difftodraft.keys()))
    ui = repo.ui
    unfi = repo.unfiltered()
    mutationentries = []
    tohide = set()
    for diffid, draftnodes in sorted(difftodraft.items()):
        publicnode = difftopublic.get(diffid)
        if publicnode is None or publicnode not in unfi:
            continue
        # sanity check - the public commit should have a sane commit message.
        if diffprops.parserevfromcommitmsg(unfi[publicnode].description()) != diffid:
            continue
        draftnodestr = ", ".join(short(d) for d in draftnodes)
        ui.status(
            _("marking D%s (%s) as landed as %s\n")
            % (diffid, draftnodestr, short(publicnode))
        )
        for draftnode in draftnodes:
            tohide.add(draftnode)
            skipnodes.add(draftnode)
            mutationentries.append(
                mutation.createsyntheticentry(
                    unfi, mutation.ORIGIN_SYNTHETIC, [draftnode], publicnode, "land"
                )
            )
    if not tohide:
        return
    if not dryrun:
        with unfi.lock(), unfi.transaction("pullcreatemarkers"):
            if mutation.recording(unfi):
                mutation.recordentries(unfi, mutationentries, skipexisting=False)
            if visibility.tracking(unfi):
                visibility.remove(unfi, tohide)
    # In case the graphql result is paginated, query again to fetch the
    # remaining results.
    _cleanuplanded(repo, dryrun=dryrun, skipnodes=skipnodes)


@command("debugmarklanded", commands.dryrunopts)
def debugmarklanded(ui, repo, **opts):
    """query Phabricator and mark landed commits"""
    dryrun = opts.get("dry_run")
    _cleanuplanded(repo, dryrun=dryrun)
    if dryrun:
        ui.status(_("(this is a dry-run, nothing was actually done)\n"))


def getdiff(rev):
    phabrev = diffprops.parserevfromcommitmsg(rev.description())
    return int(phabrev) if phabrev else None


def extsetup(ui):
    extensions.wrapcommand(commands.table, "pull", _pull)


def _pull(orig, ui, repo, *args, **opts):
    if (
        not obsolete.isenabled(repo, obsolete.createmarkersopt)
        and not mutation.recording(repo)
        and not visibility.tracking(repo)
    ):
        return orig(ui, repo, *args, **opts)

    maxrevbeforepull = len(repo.changelog)
    r = orig(ui, repo, *args, **opts)
    maxrevafterpull = len(repo.changelog)

    if ui.configbool("pullcreatemarkers", "use-graphql"):
        _cleanuplanded(repo)
    else:
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

    unfi = repo.unfiltered()
    with unfi.lock(), unfi.transaction("pullcreatemarkers"):
        if obsolete.isenabled(repo, obsolete.createmarkersopt):
            obsolete.createmarkers(unfi, tocreate)
        if mutation.recording(repo) or visibility.tracking(repo):
            mutationentries = []
            tohide = []
            for (pred, succs) in tocreate:
                if succs and not mutation.lookup(unfi, succs[0].node()):
                    mutationentries.append(
                        mutation.createsyntheticentry(
                            unfi,
                            mutation.ORIGIN_SYNTHETIC,
                            [pred.node()],
                            succs[0].node(),
                            "land",
                        )
                    )
                tohide.append(pred.node())
            if mutation.recording(unfi):
                mutation.recordentries(unfi, mutationentries, skipexisting=False)
            if visibility.tracking(unfi):
                visibility.remove(unfi, tohide)


def getlandeddiffs(repo, start, stop, onlypublic=True):
    landeddiffs = {}

    for rev in range(start, stop):
        if rev not in repo:
            # it may be hidden (e.g. a snapshot rev)
            continue
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

    for rev in unfiltered.revs("draft() - obsolete() - hidden()"):
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
