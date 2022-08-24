# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""
mark commits as "Landed" on pull

Config::

    [pullcreatemarkers]
    # Use graphql to query what diffs are landed, instead of scanning
    # through pulled commits.
    use-graphql = true

    # Make sure commits being hidden matches the commit hashes in
    # Phabricator. Locally modified commits won't be hidden.
    check-local-versions = true

    # Hook the pull command to preform a "mark landed" operation.
    # Note: This is suspected to hide commits unexpectedly.
    # It is currently only useful for test compatibility.
    hook-pull = true
"""
from .. import commands, extensions, mutation, phases, registrar, visibility
from ..i18n import _, _n
from ..node import short
from .extlib.phabricator import arcconfig, diffprops, graphql
from .phabstatus import COMMITTEDSTATUS, getdiffstatus


cmdtable = {}
command = registrar.command(cmdtable)

configtable = {}
configitem = registrar.configitem(configtable)
configitem("pullcreatemarkers", "check-local-versions", default=False)
configitem("pullcreatemarkers", "hook-pull", default=False)
configitem("pullcreatemarkers", "use-graphql", default=True)


def _isrevert(message, diffid):
    result = ("Revert D%s" % diffid) in message
    return result


def _cleanuplanded(repo, dryrun=False):
    """Query Phabricator about states of draft commits and optionally mark them
    as landed.

    This uses mutation and visibility directly.
    """
    ui = repo.ui
    try:
        client = graphql.Client(repo=repo)
    except arcconfig.ArcConfigError:
        # Not all repos have arcconfig. If a repo doesn't have one, that's not
        # a fatal error.
        return
    except Exception as ex:
        ui.warn(
            _(
                "warning: failed to initialize GraphQL client (%r), not marking commits as landed\n"
            )
            % ex
        )
        return

    limit = repo.ui.configint("pullcreatemarkers", "diff-limit", 100)
    difftodraft = {}  # {str: {node}}
    for ctx in repo.set("sort(draft() - obsolete(), -rev)"):
        diffid = diffprops.parserevfromcommitmsg(ctx.description())  # str or None
        if diffid and not _isrevert(ctx.description(), diffid):
            difftodraft.setdefault(diffid, set()).add(ctx.node())
            # Bound the number of diffs we query from Phabricator.
            if len(difftodraft) >= limit:
                break

    try:
        difftopublic, difftolocal = client.getlandednodes(
            repo, list(difftodraft.keys())
        )
    except KeyboardInterrupt:
        ui.warn(
            _(
                "reading from Phabricator was interrupted, not marking commits as landed\n"
            )
        )
        return
    except Exception as ex:
        ui.warn(
            _(
                "warning: failed to read from Phabricator for landed commits (%r), not marking commits as landed\n"
            )
            % ex
        )
        return
    unfi = repo
    mutationentries = []
    tohide = set()
    markedcount = 0
    checklocalversions = ui.configbool("pullcreatemarkers", "check-local-versions")
    for diffid, draftnodes in sorted(difftodraft.items()):
        publicnode = difftopublic.get(diffid)
        if checklocalversions:
            draftnodes = draftnodes & difftolocal.get(diffid, set())
        if publicnode is None or publicnode not in unfi:
            continue
        # skip it if the local repo does not think it's a public commit.
        if unfi[publicnode].phase() != phases.public:
            continue
        # sanity check - the public commit should have a sane commit message.
        if diffprops.parserevfromcommitmsg(unfi[publicnode].description()) != diffid:
            continue
        draftnodestr = ", ".join(
            short(d) for d in sorted(draftnodes)
        )  # making output deterministic
        if ui.verbose:
            ui.write(
                _("marking D%s (%s) as landed as %s\n")
                % (diffid, draftnodestr, short(publicnode))
            )
        markedcount += len(draftnodes)
        for draftnode in draftnodes:
            tohide.add(draftnode)
            mutationentries.append(
                mutation.createsyntheticentry(unfi, [draftnode], publicnode, "land")
            )
    if markedcount:
        ui.status(
            _n(
                "marked %d commit as landed\n",
                "marked %d commits as landed\n",
                markedcount,
            )
            % markedcount
        )
    if not tohide:
        return
    if not dryrun:
        with unfi.lock(), unfi.transaction("pullcreatemarkers"):
            # Any commit hash's added to the idmap in the earlier code will have
            # been dropped by the repo.invalidate() that happens at lock time.
            # Let's refetch those hashes now. If we don't then the
            # mutation/obsolete computation will fail to consider this mutation
            # marker, since it ignores markers for which we don't have the hash
            # for the mutation target.
            unfi.changelog.filternodes(list(e.succ() for e in mutationentries))
            if mutation.enabled(unfi):
                mutation.recordentries(unfi, mutationentries, skipexisting=False)
            if visibility.tracking(unfi):
                visibility.remove(unfi, tohide)


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
    if ui.configbool("pullcreatemarkers", "hook-pull"):
        extensions.wrapcommand(commands.table, "pull", _pull)


def _pull(orig, ui, repo, *args, **opts):
    if not mutation.enabled(repo) and not visibility.tracking(repo):
        return orig(ui, repo, *args, **opts)

    maxrevbeforepull = len(repo.changelog)
    r = orig(ui, repo, *args, **opts)
    maxrevafterpull = len(repo.changelog)

    # With lazy pull fast path the legacy "createmarkers" path will trigger
    # one-by-one resolution for all newly pulled commits. That's unusably slow
    # and is incompatible with the lazy pull. Force GraphQL code path in that
    # case.
    if ui.configbool("pullcreatemarkers", "use-graphql") or ui.configbool(
        "pull", "master-fastpath"
    ):
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

    unfi = repo
    with unfi.lock(), unfi.transaction("pullcreatemarkers"):
        if mutation.enabled(repo) or visibility.tracking(repo):
            mutationentries = []
            tohide = []
            for (pred, succs) in tocreate:
                if not succs:
                    continue
                mutdag = mutation.getdag(repo, succs[0].node())
                if pred.node() in mutdag.all():
                    continue
                mutationentries.append(
                    mutation.createsyntheticentry(
                        unfi, [pred.node()], succs[0].node(), "land"
                    )
                )
                tohide.append(pred.node())
            if mutation.enabled(unfi):
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
    unfiltered = repo

    for rev in unfiltered.revs("draft() - obsolete() - hidden()"):
        ctx = unfiltered[rev]
        diff = getdiff(ctx)

        if (
            diff in landeddiffs
            and not _isrevert(ctx.description(), str(diff))
            and landeddiffs[diff].rev() != ctx.rev()
        ):
            marker = (ctx, (landeddiffs[diff],))
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
