# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""
mark commits as "Landed" on pull

Config::

    [pullcreatemarkers]
    # Make sure commits being hidden matches the commit hashes in
    # Phabricator. Locally modified commits won't be hidden.
    check-local-versions = true
"""
from .. import commands, mutation, phases, registrar, visibility
from ..i18n import _, _n
from ..node import short
from .extlib.phabricator import arcconfig, diffprops, graphql


cmdtable = {}
command = registrar.command(cmdtable)

configtable = {}
configitem = registrar.configitem(configtable)
configitem("pullcreatemarkers", "check-local-versions", default=False)


def _isrevert(message, diffid):
    result = ("Revert D%s" % diffid) in message
    return result


def _cleanuplanded(repo, dryrun=False):
    """Query Phabricator about states of draft commits and optionally mark them
    as landed.

    This uses mutation and visibility directly.
    """
    ui = repo.ui
    difftodraft = _get_diff_to_draft(repo)
    query_result = _query_phabricator(
        repo, list(difftodraft.keys()), ["Closed", "Abandoned"]
    )
    if query_result is None:
        return None
    difftopublic, difftolocal, difftostatus = query_result
    mutationentries = []
    tohide = set()
    markedcount_landed = 0
    markedcount_abandoned = 0
    visible_heads = visibility.heads(repo)

    checklocalversions = ui.configbool("pullcreatemarkers", "check-local-versions")
    for diffid, draftnodes in sorted(difftodraft.items()):
        status = difftostatus.get(diffid)
        if not status:
            continue
        if status == "Closed":
            markedcount_landed += _process_landed(
                repo,
                diffid,
                draftnodes,
                difftopublic,
                difftolocal,
                checklocalversions,
                tohide,
                mutationentries,
            )
        elif status == "Abandoned":
            # filter out unhidable nodes
            draftnodes = {node for node in draftnodes if node in visible_heads}
            markedcount_abandoned += _process_abandonded(
                repo,
                diffid,
                draftnodes,
                difftolocal,
                checklocalversions,
                tohide,
            )

    if markedcount_landed:
        ui.status(
            _n(
                "marked %d commit as landed\n",
                "marked %d commits as landed\n",
                markedcount_landed,
            )
            % markedcount_landed
        )
    if markedcount_abandoned:
        ui.status(
            _n(
                "marked %d commit as abandoned\n",
                "marked %d commits as abandoned\n",
                markedcount_abandoned,
            )
            % markedcount_abandoned
        )
    _hide_commits(repo, tohide, mutationentries, dryrun)


def _get_diff_to_draft(repo):
    limit = repo.ui.configint("pullcreatemarkers", "diff-limit", 100)
    difftodraft = {}  # {str: {node}}
    for ctx in repo.set("sort(draft() - obsolete(), -rev)"):
        diffid = diffprops.parserevfromcommitmsg(ctx.description())  # str or None
        if diffid and not _isrevert(ctx.description(), diffid):
            difftodraft.setdefault(diffid, set()).add(ctx.node())
            # Bound the number of diffs we query from Phabricator.
            if len(difftodraft) >= limit:
                break
    return difftodraft


def _query_phabricator(repo, diffids, diff_status_list):
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

    try:

        return client.getnodes(repo, diffids, diff_status_list)

    except Exception as ex:
        ui.warn(
            _(
                "warning: failed to read from Phabricator for landed commits (%r), not marking commits as landed\n"
            )
            % ex
        )


def _process_abandonded(
    repo,
    diffid,
    draftnodes,
    difftolocal,
    checklocalversions,
    tohide,
):
    ui = repo.ui
    if checklocalversions:
        draftnodes = draftnodes & difftolocal.get(diffid, set())
    draftnodestr = ", ".join(short(d) for d in sorted(draftnodes))
    if ui.verbose and draftnodestr:
        ui.write(_("marking D%s (%s) as abandoned\n") % (diffid, draftnodestr))
    tohide |= set(draftnodes)
    return len(draftnodes)


def _process_landed(
    repo,
    diffid,
    draftnodes,
    difftopublic,
    difftolocal,
    checklocalversions,
    tohide,
    mutationentries,
):
    ui = repo.ui
    publicnode = difftopublic.get(diffid)
    if publicnode is None or publicnode not in repo:
        return 0
    # skip it if the local repo does not think it's a public commit.
    if repo[publicnode].phase() != phases.public:
        return 0
    # sanity check - the public commit should have a sane commit message.
    if diffprops.parserevfromcommitmsg(repo[publicnode].description()) != diffid:
        return 0

    if checklocalversions:
        draftnodes = draftnodes & difftolocal.get(diffid, set())
    draftnodestr = ", ".join(
        short(d) for d in sorted(draftnodes)
    )  # making output deterministic
    if ui.verbose:
        ui.write(
            _("marking D%s (%s) as landed as %s\n")
            % (diffid, draftnodestr, short(publicnode))
        )
    for draftnode in draftnodes:
        tohide.add(draftnode)
        mutationentries.append(
            mutation.createsyntheticentry(repo, [draftnode], publicnode, "land")
        )

    return len(draftnodes)


def _hide_commits(repo, tohide, mutationentries, dryrun):
    if not tohide:
        return
    if not dryrun:
        with repo.lock(), repo.transaction("pullcreatemarkers"):
            # Any commit hash's added to the idmap in the earlier code will have
            # been dropped by the repo.invalidate() that happens at lock time.
            # Let's refetch those hashes now. If we don't then the
            # mutation/obsolete computation will fail to consider this mutation
            # marker, since it ignores markers for which we don't have the hash
            # for the mutation target.
            repo.changelog.filternodes(list(e.succ() for e in mutationentries))
            if mutation.enabled(repo):
                mutation.recordentries(repo, mutationentries, skipexisting=False)
            if visibility.tracking(repo):
                visibility.remove(repo, tohide)


@command("debugmarklanded", commands.dryrunopts)
def debugmarklanded(ui, repo, **opts):
    """query Phabricator and mark landed commits"""
    dryrun = opts.get("dry_run")
    _cleanuplanded(repo, dryrun=dryrun)
    if dryrun:
        ui.status(_("(this is a dry-run, nothing was actually done)\n"))


def uisetup(ui):
    ui.setconfig("hooks", "post-pull.marklanded", _("@prog@ debugmarklanded"))
