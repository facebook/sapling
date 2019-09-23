# Copyright 2018-present Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import itertools
import re
import socket
import time

from edenscm.mercurial import (
    blackbox,
    exchange,
    extensions,
    hg,
    hintutil,
    node as nodemod,
    obsolete,
    perftrace,
    progress,
    util,
    visibility,
)
from edenscm.mercurial.i18n import _

from . import (
    backup,
    backupbookmarks,
    backuplock,
    backupstate,
    error as ccerror,
    obsmarkers as obsmarkersmod,
    service,
    subscription,
    syncstate,
    token,
    util as ccutil,
    workspace,
)


def _isremotebookmarkssyncenabled(ui):
    return ui.configbool("remotenames", "selectivepull") and ui.configbool(
        "commitcloud", "remotebookmarkssync"
    )


def _getheads(repo):
    if visibility.enabled(repo):
        return [nodemod.hex(n) for n in visibility.heads(repo)]
    else:
        # Select the commits to sync.  To match previous behaviour, this is
        # all draft but not obsolete commits, plus any bookmarked commits,
        # and all of their ancestors.
        headsrevset = repo.set(
            "heads(draft() & ::((draft() & not obsolete()) + bookmark()))"
        )
        return [ctx.hex() for ctx in headsrevset]


def _getbookmarks(repo):
    return {n: nodemod.hex(v) for n, v in repo._bookmarks.items()}


def _getremotebookmarks(repo):
    if not _isremotebookmarkssyncenabled(repo.ui):
        return {}

    remotebookmarks = {}
    if util.safehasattr(repo, "names") and "remotebookmarks" in repo.names:
        ns = repo.names["remotebookmarks"]
        rbooknames = ns.listnames(repo)
        for book in rbooknames:
            nodes = ns.namemap(repo, book)
            if nodes:
                remotebookmarks[book] = nodemod.hex(nodes[0])
    return remotebookmarks


@perftrace.tracefunc("Cloud Sync")
def sync(
    repo, remotepath, getconnection, cloudrefs=None, full=False, cloudversion=None
):
    ui = repo.ui
    start = util.timer()

    startnode = repo["."].node()

    if full:
        maxage = None
    else:
        maxage = ui.configint("commitcloud", "max_sync_age", None)

    # Work out which repo and workspace we are synchronizing with.
    reponame = ccutil.getreponame(repo)
    workspacename = workspace.currentworkspace(repo)
    if workspacename is None:
        raise ccerror.WorkspaceError(ui, _("undefined workspace"))

    # Connect to the commit cloud service.
    tokenlocator = token.TokenLocator(ui)
    serv = service.get(ui, tokenlocator.token)

    ui.status(
        _("synchronizing '%s' with '%s'\n") % (reponame, workspacename),
        component="commitcloud",
    )
    backuplock.progress(repo, "starting synchronizing with '%s'" % workspacename)

    # Work out what version to fetch updates from.
    lastsyncstate = syncstate.SyncState(repo, workspacename)
    fetchversion = lastsyncstate.version
    if maxage != lastsyncstate.maxage:
        # We are doing a full sync, or maxage has changed since the last sync,
        # so get a fresh copy of the full state.
        fetchversion = 0

    # External services may already know the version number.  Check if we're
    # already up-to-date.
    if cloudversion is not None and cloudversion <= lastsyncstate.version:
        ui.status(
            _("this version has been already synchronized\n"), component="commitcloud"
        )
        return 0

    backupsnapshots = False
    try:
        extensions.find("snapshot")
        backupsnapshots = True
    except KeyError:
        pass

    origheads = _getheads(repo)
    origbookmarks = _getbookmarks(repo)

    # Back up all local commits that are not already backed up.
    # Load the backup state under the repo lock to ensure a consistent view.
    with repo.lock():
        state = backupstate.BackupState(repo, remotepath)
    backedup, failed = backup.backup(
        repo, state, remotepath, getconnection, backupsnapshots=backupsnapshots
    )

    # On cloud rejoin we already know what the cloudrefs are.  Otherwise,
    # fetch them from the commit cloud service.
    if cloudrefs is None:
        cloudrefs = serv.getreferences(reponame, workspacename, fetchversion)

    with repo.ui.configoverride(
        {("treemanifest", "prefetchdraftparents"): False}, "cloudsync"
    ), repo.wlock(), repo.lock(), repo.transaction("cloudsync") as tr:

        if origheads != _getheads(repo) or origbookmarks != _getbookmarks(repo):
            # Another transaction changed the repository while we were backing
            # up commits. This may have introduced new commits that also need
            # backing up.  That transaction should have started its own sync
            # process, so give up on this sync, and let the later one perform
            # the sync.
            raise ccerror.SynchronizationError(ui, _("repo changed while backing up"))

        synced = False
        while not synced:

            # Apply any changes from the cloud to the local repo.
            if cloudrefs.version != fetchversion:
                _applycloudchanges(
                    repo, remotepath, lastsyncstate, cloudrefs, maxage, state, tr
                )

            # Check if any omissions are now included in the repo
            _checkomissions(repo, remotepath, lastsyncstate)

            # Send updates to the cloud.  If this fails then we have lost the race
            # to update the server and must start again.
            synced, cloudrefs = _submitlocalchanges(
                repo, reponame, workspacename, lastsyncstate, failed, serv
            )

    # Update the backup bookmarks with any changes we have made by syncing.
    backupbookmarks.pushbackupbookmarks(repo, remotepath, getconnection, state)

    backuplock.progresscomplete(repo)

    if failed:
        failedset = set(repo.nodes("%ld::", failed))
        if len(failedset) == 1:
            repo.ui.warn(
                _("failed to synchronize %s\n") % nodemod.short(failedset.pop()),
                component="commitcloud",
            )
        else:
            repo.ui.warn(
                _("failed to synchronize %d commits\n") % len(failedset),
                component="commitcloud",
            )
    else:
        ui.status(_("commits synchronized\n"), component="commitcloud")

    elapsed = util.timer() - start
    ui.status(_("finished in %0.2f sec\n") % elapsed)

    # Check that Scm Service is running and a subscription exists
    subscription.check(repo)

    # log whether the sync was successful
    with repo.wlock():
        fp = repo.localvfs("lastsync.log", "w+")
        if synced and not failed:
            fp.write("Success")
        else:
            fp.write("Failed")
        fp.close()
    return _maybeupdateworkingcopy(repo, startnode)


def logsyncop(repo, op, version, oldheads, newheads, oldbm, newbm, oldrbm, newrbm):
    oldheadsset = set(oldheads)
    newheadsset = set(newheads)
    oldbmset = set(oldbm)
    newbmset = set(newbm)
    oldrbmset = set(oldrbm)
    newrbmset = set(newrbm)
    addedheads = blackbox.shortlist([h for h in newheads if h not in oldheadsset])
    removedheads = blackbox.shortlist([h for h in oldheads if h not in newheadsset])
    addedbm = blackbox.shortlist([h for h in newbm if h not in oldbmset])
    removedbm = blackbox.shortlist([h for h in oldbm if h not in newbmset])
    addedrbm = blackbox.shortlist([h for h in newrbm if h not in oldrbmset])
    removedrbm = blackbox.shortlist([h for h in oldrbm if h not in newrbmset])
    blackbox.log(
        {
            "commit_cloud_sync": {
                "op": op,
                "version": version,
                "added_heads": addedheads,
                "removed_heads": removedheads,
                "added_bookmarks": addedbm,
                "removed_bookmarks": removedbm,
                "added_remote_bookmarks": addedrbm,
                "removed_remote_bookmarks": removedrbm,
            }
        }
    )


def _maybeupdateworkingcopy(repo, currentnode):
    ui = repo.ui

    if repo["."].node() != currentnode:
        return 0

    successors = list(repo.nodes("successors(%n) - obsolete()", currentnode))

    if len(successors) == 0:
        return 0

    if len(successors) == 1:
        destination = successors[0]
        if destination not in repo or destination == currentnode:
            return 0
        ui.status(
            _("current revision %s has been moved remotely to %s\n")
            % (nodemod.short(currentnode), nodemod.short(destination)),
            component="commitcloud",
        )
        if ui.configbool("commitcloud", "updateonmove"):
            if repo[destination].mutable():
                backuplock.progress(
                    repo,
                    "updating %s from %s to %s"
                    % (
                        repo.wvfs.base,
                        nodemod.short(currentnode),
                        nodemod.short(destination),
                    ),
                )
                ui.status(_("updating to %s\n") % nodemod.short(destination))
                return hg.updatetotally(
                    ui, repo, destination, destination, updatecheck="noconflict"
                )
        else:
            hintutil.trigger("commitcloud-update-on-move")
    else:
        ui.status(
            _(
                "current revision %s has been replaced remotely with multiple revisions\n"
                "(run 'hg update HASH' to go to the desired revision)\n"
            )
            % nodemod.short(currentnode),
            component="commitcloud",
        )
    return 0


@perftrace.tracefunc("Apply Cloud Changes")
def _applycloudchanges(repo, remotepath, lastsyncstate, cloudrefs, maxage, state, tr):
    pullcmd, pullopts = ccutil.getcommandandoptions("^pull")

    try:
        remotenames = extensions.find("remotenames")
    except KeyError:
        remotenames = None

    # Pull all the new heads and any bookmark hashes we don't have. We need to
    # filter cloudrefs before pull as pull doesn't check if a rev is present
    # locally.
    unfi = repo.unfiltered()
    newheads = [head for head in cloudrefs.heads if head not in unfi]
    if maxage is not None and maxage >= 0:
        mindate = time.time() - maxage * 86400
        omittedheads = [
            head
            for head in newheads
            if head in cloudrefs.headdates and cloudrefs.headdates[head] < mindate
        ]
        if omittedheads:
            repo.ui.status(_("omitting heads that are older than %d days:\n") % maxage)
            for head in omittedheads:
                headdatestr = util.datestr(util.makedate(cloudrefs.headdates[head]))
                repo.ui.status(_("  %s from %s\n") % (head[:12], headdatestr))
        newheads = [head for head in newheads if head not in omittedheads]
    else:
        omittedheads = []
    omittedbookmarks = []

    newvisibleheads = None
    if visibility.tracking(repo):
        localheads = _getheads(repo)
        localheadsset = set(localheads)
        cloudheads = [head for head in cloudrefs.heads if head not in omittedheads]
        cloudheadsset = set(cloudheads)
        if localheadsset != cloudheadsset:
            oldvisibleheads = [
                head
                for head in lastsyncstate.heads
                if head not in lastsyncstate.omittedheads
            ]
            newvisibleheads = util.removeduplicates(
                oldvisibleheads + cloudheads + localheads
            )
            toremove = {
                head
                for head in oldvisibleheads
                if head not in localheadsset or head not in cloudheadsset
            }
            newvisibleheads = [head for head in newvisibleheads if head not in toremove]

    remotebookmarknodes = []
    newremotebookmarks = {}
    if _isremotebookmarkssyncenabled(repo.ui):
        newremotebookmarks = _processremotebookmarks(
            repo, cloudrefs.remotebookmarks, lastsyncstate
        )

        # Pull public commits, which remote bookmarks point to, if they are not
        # present locally.
        for node in newremotebookmarks.values():
            if node not in unfi:
                remotebookmarknodes.append(node)

    backuplock.progresspulling(repo, [nodemod.bin(node) for node in newheads])

    if remotebookmarknodes or newheads:
        # Partition the heads into groups we can pull together.
        headgroups = (
            [remotebookmarknodes] if remotebookmarknodes else []
        ) + _partitionheads(newheads, cloudrefs.headdates)

        def disabled(*args, **kwargs):
            pass

        # Disable pulling of obsmarkers
        wrapobs = extensions.wrappedfunction(exchange, "_pullobsolete", disabled)

        # Disable pulling of bookmarks
        wrapbook = extensions.wrappedfunction(exchange, "_pullbookmarks", disabled)

        # Disable pulling of remote bookmarks
        if remotenames:
            wrapremotenames = extensions.wrappedfunction(
                remotenames, "pullremotenames", disabled
            )
        else:
            wrapremotenames = util.nullcontextmanager()

        # Disable automigration and prefetching of trees
        configoverride = repo.ui.configoverride(
            {("pull", "automigrate"): False, ("treemanifest", "pullprefetchrevs"): ""},
            "cloudsyncpull",
        )

        prog = progress.bar(
            repo.ui, _("pulling from commit cloud"), total=len(headgroups)
        )
        with wrapobs, wrapbook, wrapremotenames, configoverride, prog:
            for index, headgroup in enumerate(headgroups):
                headgroupstr = " ".join([head[:12] for head in headgroup])
                repo.ui.status(_("pulling %s\n") % headgroupstr)
                prog.value = (index, headgroupstr)
                pullopts["rev"] = headgroup
                pullcmd(repo.ui, repo, remotepath, **pullopts)
                repo.connectionpool.close()

    omittedbookmarks.extend(
        _mergebookmarks(repo, tr, cloudrefs.bookmarks, lastsyncstate)
    )

    if _isremotebookmarkssyncenabled(repo.ui):
        _updateremotebookmarks(repo, tr, newremotebookmarks)

    _mergeobsmarkers(repo, tr, cloudrefs.obsmarkers)

    if newvisibleheads is not None:
        visibility.setvisibleheads(repo, [nodemod.bin(n) for n in newvisibleheads])

    # Obsmarker sharing is unreliable.  Some of the commits that should now
    # be visible might be hidden still, and some commits that should be
    # hidden might still be visible.  Create local obsmarkers to resolve
    # this.
    if obsolete.isenabled(repo, obsolete.createmarkersopt):
        unfi = repo.unfiltered()
        # Commits that are only visible in the cloud are commits that are
        # ancestors of the cloud heads but are hidden locally.
        cloudvisibleonly = list(
            unfi.set(
                "not public() & ::%ls & hidden()",
                [head for head in cloudrefs.heads if head not in omittedheads],
            )
        )
        # Commits that are only hidden in the cloud are commits that are
        # ancestors of the previous cloud heads that are not ancestors of the
        # current cloud heads, but have not been hidden or obsoleted locally.
        cloudhiddenonly = list(
            unfi.set(
                "(not public() & ::%ls) - (not public() & ::%ls) - hidden() - obsolete()",
                [
                    head
                    for head in lastsyncstate.heads
                    if head not in lastsyncstate.omittedheads
                ],
                [head for head in cloudrefs.heads if head not in omittedheads],
            )
        )
        if cloudvisibleonly or cloudhiddenonly:
            msg = _(
                "detected obsmarker inconsistency (fixing by obsoleting [%s] and reviving [%s])\n"
            ) % (
                ", ".join([nodemod.short(ctx.node()) for ctx in cloudhiddenonly]),
                ", ".join([nodemod.short(ctx.node()) for ctx in cloudvisibleonly]),
            )
            repo.ui.log("commitcloud_sync", msg)
            repo.ui.warn(msg)
            repo._commitcloudskippendingobsmarkers = True
            with repo.lock():
                obsolete.createmarkers(repo, [(ctx, ()) for ctx in cloudhiddenonly])
                obsolete.revive(cloudvisibleonly)
            repo._commitcloudskippendingobsmarkers = False

    # We have now synced the repo to the cloud version.  Store this.
    logsyncop(
        repo,
        "from_cloud",
        cloudrefs.version,
        lastsyncstate.heads,
        cloudrefs.heads,
        lastsyncstate.bookmarks,
        cloudrefs.bookmarks,
        lastsyncstate.remotebookmarks,
        newremotebookmarks,
    )
    lastsyncstate.update(
        cloudrefs.version,
        cloudrefs.heads,
        cloudrefs.bookmarks,
        omittedheads,
        omittedbookmarks,
        maxage,
        newremotebookmarks,
    )

    # Also update backup state.  These new heads are already backed up,
    # otherwise the server wouldn't have told us about them.
    state.update([nodemod.bin(head) for head in newheads], tr)


def _partitionheads(heads, headdates, sizelimit=4, spanlimit=86400):
    """partition a list of heads into groups limited by size and timespan

    Partitions the list of heads into a list of head groups.  Each head group
    contains at most sizelimit heads, and all the heads have a date within
    spanlimit of each other in the headdates map.

    The head ordering is preserved, as we want to pull commits in the same order
    so that order-dependent views like smartlog match as closely as possible on
    different synced machines.  This may mean potential groups get split up if a
    head with a different date is in the middle.

    >>> _partitionheads([1, 2, 3, 4], {1: 1, 2: 2, 3: 3, 4: 4}, sizelimit=2, spanlimit=10)
    [[1, 2], [3, 4]]
    >>> _partitionheads([1, 2, 3, 4], {1: 10, 2: 20, 3: 30, 4: 40}, sizelimit=4, spanlimit=10)
    [[1, 2], [3, 4]]
    >>> _partitionheads([1, 2, 3, 4], {1: 10, 2: 20, 3: 30, 4: 40}, sizelimit=4, spanlimit=30)
    [[1, 2, 3, 4]]
    >>> _partitionheads([1, 2, 3, 4], {1: 10, 2: 20, 3: 30, 4: 40}, sizelimit=4, spanlimit=5)
    [[1], [2], [3], [4]]
    >>> _partitionheads([1, 2, 3, 9, 4], {1: 10, 2: 20, 3: 30, 4: 40, 9: 90}, sizelimit=8, spanlimit=30)
    [[1, 2, 3], [9], [4]]
    """
    headgroups = []
    headsbydate = [(headdates.get(head, 0), head) for head in heads]
    headgroup = None
    groupstartdate = None
    groupenddate = None
    for date, head in headsbydate:
        if (
            headgroup is None
            or len(headgroup) >= sizelimit
            or date < groupstartdate
            or date > groupenddate
        ):
            if headgroup:
                headgroups.append(headgroup)
            headgroup = []
            groupstartdate = date - spanlimit
            groupenddate = date + spanlimit
        headgroup.append(head)
        groupstartdate = max(groupstartdate, date - spanlimit)
        groupenddate = min(groupenddate, date + spanlimit)
    if headgroup:
        headgroups.append(headgroup)
    return headgroups


def _processremotebookmarks(repo, cloudremotebooks, lastsyncstate):
    """calculate new state between the cloud remote bookmarks and the local
    remote bookmarks

    Performs a 3-way diff between the last sync remote bookmark state, new cloud
    state and local remote bookmarks.

    Returns a dict <remotebookmark: newnode> - new state of remote bookmarks"""

    def usecloudnode(cloudnode, localnode):
        """returns True if cloudnode should be a new state for the remote bookmark

        Both cloudnode and localnode are public commits."""
        unfi = repo.unfiltered()
        if localnode not in unfi:
            # we somehow don't have the localnode in the repo, probably may want
            # to fetch it
            return False
        if cloudnode not in unfi:
            # we don't have cloudnode in the repo, assume that cloudnode is newer
            # than the local
            return True
        if repo.changelog.isancestor(nodemod.bin(localnode), nodemod.bin(cloudnode)):
            # cloudnode is descendant of the localnode, assume that remote book
            # should move forward to the newer node
            #
            # Note: if remote book was reverted back to the older revision on
            # the server, and current repo in fact has newer working copy, then
            # we'll end up with wrong state by moving the bookmark forward.
            # It will be fixed on the next pull and sync operations.
            return True
        return False

    localremotebooks = _getremotebookmarks(repo)
    oldcloudremotebooks = lastsyncstate.remotebookmarks
    allremotenames = set(localremotebooks.keys() + cloudremotebooks.keys())

    newremotebooks = {}
    for remotename in allremotenames:
        cloudnode = cloudremotebooks.get(remotename, None)
        localnode = localremotebooks.get(remotename, None)
        oldcloudnode = oldcloudremotebooks.get(remotename, None)

        if (
            cloudnode != localnode
            and cloudnode != oldcloudnode
            and localnode != oldcloudnode
        ):
            # Both cloud [remote bookmark -> node] mapping and local have changed.
            if cloudnode and localnode:
                newremotebooks[remotename] = (
                    cloudnode if usecloudnode(cloudnode, localnode) else localnode
                )
            else:
                # The remote bookmark was deleted in the cloud or in the current
                # repo. Keep local remote book for now: if it was deleted on the
                # server, the state will be updated with the next pull.
                # (Unsubscription mechanism is not implemented yet)
                #
                # Note: consider 'deleted' status in the Cloud table
                newremotebooks[remotename] = localnode
        elif cloudnode == localnode:
            # if both were updated to the same place
            if cloudnode:
                newremotebooks[remotename] = localnode
            continue

        if cloudnode and cloudnode != oldcloudnode:
            # Cloud has changes, need to apply them
            newremotebooks[remotename] = cloudnode

        if localnode and localnode != oldcloudnode:
            # Need to update the cloud
            newremotebooks[remotename] = localnode

    return newremotebooks


def _updateremotebookmarks(repo, tr, remotebookmarks):
    """updates the remote bookmarks to point their new nodes"""
    repo._remotenames.applychanges({"bookmarks": remotebookmarks})


def _mergebookmarks(repo, tr, cloudbookmarks, lastsyncstate):
    """merge any changes to the cloud bookmarks with any changes to local ones

    This performs a 3-way diff between the old cloud bookmark state, the new
    cloud bookmark state, and the local bookmark state.  If either local or
    cloud bookmarks have been modified, propagate those changes to the other.
    If both have been modified then fork the bookmark by renaming the local one
    and accepting the cloud bookmark's new value.

    Some of the bookmark changes may not be possible to apply, as the bookmarked
    commit has been omitted locally.  In that case the bookmark is omitted.

    Returns a list of the omitted bookmark names.
    """
    unfi = repo.unfiltered()
    localbookmarks = _getbookmarks(repo)
    omittedbookmarks = set(lastsyncstate.omittedbookmarks)
    changes = []
    allnames = set(localbookmarks.keys() + cloudbookmarks.keys())
    newnames = set()
    for name in allnames:
        # We are doing a 3-way diff between the local bookmark and the cloud
        # bookmark, using the previous cloud bookmark's value as the common
        # ancestor.
        localnode = localbookmarks.get(name)
        cloudnode = cloudbookmarks.get(name)
        lastcloudnode = lastsyncstate.bookmarks.get(name)
        if cloudnode != localnode:
            # The local and cloud bookmarks differ, so we must merge them.

            # First, check if there is a conflict.
            if (
                localnode is not None
                and cloudnode is not None
                and localnode != lastcloudnode
                and cloudnode != lastcloudnode
            ):
                # The bookmark has changed both locally and remotely.  Fork the
                # bookmark by renaming the local one.
                forkname = _forkname(repo.ui, name, allnames | newnames)
                newnames.add(forkname)
                changes.append((forkname, nodemod.bin(localnode)))
                repo.ui.warn(
                    _(
                        "%s changed locally and remotely, "
                        "local bookmark renamed to %s\n"
                    )
                    % (name, forkname)
                )

            # If the cloud bookmarks has changed, we must apply its changes
            # locally.
            if cloudnode != lastcloudnode:
                if cloudnode is not None:
                    # The cloud bookmark has been set to point to a new commit.
                    if cloudnode in unfi:
                        # The commit is available locally, so update the
                        # bookmark.
                        changes.append((name, nodemod.bin(cloudnode)))
                        omittedbookmarks.discard(name)
                    else:
                        # The commit is not available locally.  Omit it.
                        repo.ui.warn(
                            _("%s not found, omitting %s bookmark\n")
                            % (cloudnode, name)
                        )
                        omittedbookmarks.add(name)
                        if name in localbookmarks:
                            changes.append((name, None))
                else:
                    # The bookmarks has been deleted in the cloud.
                    if localnode is not None and localnode != lastcloudnode:
                        # Although it has been deleted in the cloud, it has
                        # been moved in the repo at the same time.  Allow the
                        # local bookmark to persist - this will mean it is
                        # resurrected at the new local location.
                        pass
                    else:
                        # Remove the bookmark locally.
                        changes.append((name, None))

    repo._bookmarks.applychanges(repo, tr, changes)
    return list(omittedbookmarks)


def _forkname(ui, name, othernames):
    hostname = ui.config("commitcloud", "hostname", socket.gethostname())

    # Strip off any old suffix.
    m = re.match("-%s(-[0-9]*)?$" % re.escape(hostname), name)
    if m:
        suffix = "-%s%s" % (hostname, m.group(1) or "")
        name = name[0 : -len(suffix)]

    # Find a new name.
    for n in itertools.count():
        candidate = "%s-%s%s" % (name, hostname, "-%s" % n if n != 0 else "")
        if candidate not in othernames:
            return candidate


def _mergeobsmarkers(repo, tr, obsmarkers):
    if obsolete.isenabled(repo, obsolete.createmarkersopt):
        tr._commitcloudskippendingobsmarkers = True
        repo.obsstore.add(tr, obsmarkers)
        repo.filteredrevcache.clear()


@perftrace.tracefunc("Check Omissions")
def _checkomissions(repo, remotepath, lastsyncstate):
    """check omissions are still not available locally

    Check that the commits that have been deliberately omitted are still not
    available locally.  If they are now available (e.g. because the user pulled
    them manually), then remove the tracking of those heads being omitted, and
    restore any bookmarks that can now be restored.
    """
    unfi = repo.unfiltered()
    lastomittedheads = set(lastsyncstate.omittedheads)
    lastomittedbookmarks = set(lastsyncstate.omittedbookmarks)
    omittedheads = set()
    omittedbookmarks = set()
    changes = []
    for head in lastomittedheads:
        if head not in repo:
            omittedheads.add(head)
    for name in lastomittedbookmarks:
        # bookmark might be removed from cloud workspace by someone else
        if name not in lastsyncstate.bookmarks:
            continue
        node = lastsyncstate.bookmarks[name]
        if node in unfi:
            changes.append((name, nodemod.bin(node)))
        else:
            omittedbookmarks.add(name)
    if omittedheads != lastomittedheads or omittedbookmarks != lastomittedbookmarks:
        lastsyncstate.update(
            lastsyncstate.version,
            lastsyncstate.heads,
            lastsyncstate.bookmarks,
            list(omittedheads),
            list(omittedbookmarks),
            lastsyncstate.maxage,
            lastsyncstate.remotebookmarks,
        )
    if changes:
        with repo.wlock(), repo.lock(), repo.transaction("cloudsync") as tr:
            repo._bookmarks.applychanges(repo, tr, changes)


@perftrace.tracefunc("Submit Local Changes")
def _submitlocalchanges(repo, reponame, workspacename, lastsyncstate, failed, serv):
    localheads = _getheads(repo)
    localbookmarks = _getbookmarks(repo)
    localremotebookmarks = _getremotebookmarks(repo)
    obsmarkers = obsmarkersmod.getsyncingobsmarkers(repo)

    # If any commits failed to back up, exclude them.  Revert any bookmark changes
    # that point to failed commits.
    if failed:
        localheads = [
            nodemod.hex(head)
            for head in repo.nodes("heads(draft() & ::%ls - %ld::)", localheads, failed)
        ]
        failedset = set(repo.nodes("draft() & %ld::", failed))
        for name, bookmarknode in localbookmarks.items():
            if nodemod.bin(bookmarknode) in failedset:
                if name in lastsyncstate.bookmarks:
                    localbookmarks[name] = lastsyncstate.bookmarks[name]
                else:
                    del localbookmarks[name]

    # Work out what we should have synced locally (and haven't deliberately
    # omitted)
    omittedheads = set(lastsyncstate.omittedheads)
    omittedbookmarks = set(lastsyncstate.omittedbookmarks)
    localsyncedheads = [
        head for head in lastsyncstate.heads if head not in omittedheads
    ]
    localsyncedbookmarks = {
        name: node
        for name, node in lastsyncstate.bookmarks.items()
        if name not in omittedbookmarks
    }

    remotebookmarkschanged = (
        _isremotebookmarkssyncenabled(repo.ui)
        and localremotebookmarks != lastsyncstate.remotebookmarks
    )
    if (
        set(localheads) == set(localsyncedheads)
        and localbookmarks == localsyncedbookmarks
        and not remotebookmarkschanged
        and lastsyncstate.version != 0
        and not obsmarkers
    ):
        # Nothing to send.
        return True, None

    # The local repo has changed.  We must send these changes to the
    # cloud.

    # Work out the new cloud heads and bookmarks by merging in the
    # omitted items.  We need to preserve the ordering of the cloud
    # heads so that smartlogs generally match.
    localandomittedheads = set(localheads).union(lastsyncstate.omittedheads)
    newcloudheads = util.removeduplicates(
        [head for head in lastsyncstate.heads if head in localandomittedheads]
        + localheads
    )
    newcloudbookmarks = {
        name: localbookmarks.get(name, lastsyncstate.bookmarks.get(name))
        for name in set(localbookmarks.keys()).union(lastsyncstate.omittedbookmarks)
    }

    # Work out what the new omitted heads and bookmarks are.
    newomittedheads = list(set(newcloudheads).difference(localheads))
    newomittedbookmarks = list(
        set(newcloudbookmarks.keys()).difference(localbookmarks.keys())
    )

    # Check for workspace oscillation.  This is where we try to revert the
    # workspace back to how it was immediately prior to applying the cloud
    # changes at the start of the sync.  This is usually an error caused by
    # inconsistent obsmarkers.
    if lastsyncstate.oscillating(newcloudheads, newcloudbookmarks):
        raise ccerror.SynchronizationError(
            repo.ui,
            _(
                "oscillating commit cloud workspace detected.\n"
                "check for commits that are visible in one repo but hidden in another,\n"
                "and hide or unhide those commits in all places."
            ),
        )

    oldremotebookmarks = []
    newremotebookmarks = {}
    if _isremotebookmarkssyncenabled(repo.ui):
        # do not need to submit local remote bookmarks if the feature is not enabled
        oldremotebookmarks = lastsyncstate.remotebookmarks.keys()
        newremotebookmarks = localremotebookmarks

    backuplock.progress(repo, "finishing synchronizing with '%s'" % workspacename)
    synced, cloudrefs = serv.updatereferences(
        reponame,
        workspacename,
        lastsyncstate.version,
        lastsyncstate.heads,
        newcloudheads,
        lastsyncstate.bookmarks.keys(),
        newcloudbookmarks,
        obsmarkers,
        oldremotebookmarks,
        newremotebookmarks,
    )
    if synced:
        logsyncop(
            repo,
            "to_cloud",
            cloudrefs.version,
            lastsyncstate.heads,
            newcloudheads,
            lastsyncstate.bookmarks,
            newcloudbookmarks,
            oldremotebookmarks,
            newremotebookmarks,
        )
        lastsyncstate.update(
            cloudrefs.version,
            newcloudheads,
            newcloudbookmarks,
            newomittedheads,
            newomittedbookmarks,
            lastsyncstate.maxage,
            newremotebookmarks,
        )
        obsmarkersmod.clearsyncingobsmarkers(repo)

    return synced, cloudrefs
