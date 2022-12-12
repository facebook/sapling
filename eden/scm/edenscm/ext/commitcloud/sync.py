# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import hashlib
import itertools
import re
import socket
import time

from edenscm import (
    blackbox,
    bookmarks,
    error,
    hg,
    hintutil,
    node as nodemod,
    perftrace,
    progress,
    pycompat,
    util,
    visibility,
)
from edenscm.i18n import _, _n
from edenscm.node import hex
from edenscm.pycompat import encodeutf8

from . import (
    backup,
    backuplock,
    backupstate,
    error as ccerror,
    service,
    subscription,
    syncstate,
    upload,
    util as ccutil,
    workspace,
)


# Sync status file.  Contains whether the previous sync was successful or not.
_syncstatusfile = "commitcloudsyncstatus"

_maxomittedheadsoutput = 30

_maxomittedbookmarksoutput = 30


def _isremotebookmarkssyncenabled(ui):
    return ui.configbool("remotenames", "selectivepull") and ui.configbool(
        "commitcloud", "remotebookmarkssync"
    )


def _getheads(repo):
    if visibility.enabled(repo):
        # Visible heads can contain public heads in some cases due to a known issue.
        # TODO (liubovd): remove the filer once the issue is fixed.
        return [nodemod.hex(n) for n in visibility.heads(repo) if repo[n].mutable()]
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


def _getprotectedremotebookmarks(repo):
    return bookmarks.selectivepullinitbookmarkfullnames(repo)


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


def _iscleanrepo(repo):
    """Check if there are any local changes relevant for commit cloud in the repo"""
    return not _getheads(repo) and not _getbookmarks(repo)


@perftrace.tracefunc("Cloud Sync")
def sync(repo, *args, **kwargs):
    with backuplock.lock(repo):
        try:
            besteffort = kwargs.pop("besteffort", False)
            nc = util.nullcontextmanager
            with nc() if besteffort else repo.wlock(), nc() if besteffort else repo.lock():
                rc, synced = _sync(repo, *args, **kwargs)
            if synced is not None:
                with repo.svfs(_syncstatusfile, "w+") as fp:
                    fp.write(encodeutf8("Success" if synced else "Failed"))
        except BaseException as e:
            with repo.svfs(_syncstatusfile, "w+") as fp:
                fp.write(encodeutf8("Exception:\n%s" % e))
            raise
        return rc


def _hashrepostate(repo) -> bytes:
    """hash repo states that affect commit cloud sync

    Those states are bookmarks, remotenames, visibleheads, as they are synced
    by commit cloud. Other states like tip, config, commit cloud sync states
    are not included, as they do not affect cloud sync - changes in these states
    do not trigger a cloud sync.

    The hash is used to detect repo changes.
    """
    buf = []
    with repo.wlock(), repo.lock(), repo.transaction("cloudsyncmetalog"):
        ml = repo.metalog()
    for key in ["bookmarks", "remotenames", "visibleheads"]:
        buf.append(ml.get(key) or b"")
    return hashlib.sha1(b"".join(buf)).digest()


def _sync(
    repo,
    cloudrefs=None,
    full=False,
    cloudversion=None,
    cloudworkspace=None,
    connect_opts=None,
):
    ui = repo.ui
    start = util.timer()

    startnode = repo["."].node()
    maxage = None if full else ui.configint("commitcloud", "max_sync_age", None)

    # Work out which repo and workspace we are synchronizing with.
    reponame = ccutil.getreponame(repo)
    workspacename = workspace.currentworkspace(repo)
    if workspacename is None:
        raise ccerror.WorkspaceError(ui, _("undefined workspace"))

    # External services may know the workspacename to trigger the sync
    if cloudworkspace and workspacename != cloudworkspace:
        ui.status(_("current workspace is different than the workspace to sync\n"))
        return 1, None

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
        return 0, None

    # Connect to the commit cloud service.
    serv = service.get(ui)

    origrepostate = _hashrepostate(repo)

    remotepath = ccutil.getremotepath(ui)

    getconnection = lambda: repo.connectionpool.get(
        remotepath, connect_opts, reason="cloudsync"
    )

    # Load the backup state under the repo lock to ensure a consistent view.
    usehttp = ui.configbool("commitcloud", "usehttpupload")
    with repo.lock():
        state = backupstate.BackupState(repo, usehttp=usehttp)

    with repo.ui.timesection("commitcloud_sync_push"):
        if ui.configbool("commitcloud", "usehttpupload"):
            uploaded, failed = upload.upload(repo, None, localbackupstate=state)
            # Upload returns a list of all newly uploaded heads and failed nodes (not just heads).
            # Backup returns a revset for failed. Create a revset for compatibility.
            failed = repo.revs("%ln", failed)
        else:
            # Back up all local commits that are not already backed up.
            backedup, failed = backup._backup(repo, state, remotepath, getconnection)

    # Now that commits are backed up, check that visibleheads are enabled
    # locally, and only sync if visibleheads is enabled.
    # developer config: commitcloud.requirevisibleheads
    if repo.ui.configbool("commitcloud", "requirevisibleheads", True):
        if not visibility.enabled(repo):
            raise error.Abort(_("commit cloud sync requires new-style visibility"))

    # On cloud rejoin we already know what the cloudrefs are.  Otherwise,
    # fetch them from the commit cloud service.
    if cloudrefs is None:
        cloudrefs = serv.getreferences(
            reponame,
            workspacename,
            fetchversion,
            clientinfo=service.makeclientinfo(repo, lastsyncstate),
        )

    with repo.ui.configoverride(
        {("treemanifest", "prefetchdraftparents"): False}, "cloudsync"
    ), repo.wlock(), repo.lock():

        synced = False
        attempt = 0
        while not synced:

            if attempt >= 3:
                raise ccerror.SynchronizationError(
                    ui, _("failed to sync after %s attempts") % attempt
                )
            attempt += 1

            with repo.transaction("cloudsync") as tr:

                if _hashrepostate(repo) != origrepostate:
                    # Another transaction changed the repository while we were backing
                    # up commits. This may have introduced new commits that also need
                    # backing up.  That transaction should have started its own sync
                    # process, so give up on this sync, and let the later one perform
                    # the sync.
                    raise ccerror.SynchronizationError(
                        ui, _("repo changed while backing up")
                    )

                # Apply any changes from the cloud to the local repo.
                if cloudrefs.version != fetchversion:
                    _applycloudchanges(
                        repo, remotepath, lastsyncstate, cloudrefs, maxage, state, tr
                    )
                elif (
                    _isremotebookmarkssyncenabled(repo.ui)
                    and not lastsyncstate.remotebookmarks
                ):
                    # We're up-to-date, but didn't sync remote bookmarks last time.
                    # Sync them now.
                    cloudrefs = serv.getreferences(
                        reponame,
                        workspacename,
                        0,
                        clientinfo=service.makeclientinfo(repo, lastsyncstate),
                    )
                    _forcesyncremotebookmarks(
                        repo, cloudrefs, lastsyncstate, remotepath, tr
                    )

                # Check if any omissions are now included in the repo
                _checkomissions(repo, remotepath, lastsyncstate, tr, maxage)

            # We committed the transaction so that data downloaded from the cloud is
            # committed.  Start a new transaction for uploading the local changes.
            with repo.transaction("cloudsync") as tr:

                # Send updates to the cloud.  If this fails then we have lost the race
                # to update the server and must start again.
                synced, cloudrefs = _submitlocalchanges(
                    repo, reponame, workspacename, lastsyncstate, failed, serv, tr
                )

    backuplock.progresscomplete(repo)

    if failed:
        failedset = set(repo.nodes("%ld::", failed))
        if len(failedset) < 10:
            while failedset:
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

    return _maybeupdateworkingcopy(repo, startnode), synced and not failed


def logsyncop(
    repo,
    op,
    version,
    oldheads,
    newheads,
    oldbm,
    newbm,
    oldrbm,
    newrbm,
):
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
    util.info("commit-cloud-sync", op=op, version=version)


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
                with repo.wlock(), repo.lock(), repo.transaction("sync-checkout"):
                    return hg.updatetotally(
                        ui, repo, destination, destination, updatecheck="noconflict"
                    )
        else:
            hintutil.trigger("commitcloud-update-on-move")
    else:
        ui.status(
            _(
                "current revision %s has been replaced remotely with multiple revisions\n"
                "(run '@prog@ goto HASH' to go to the desired revision)\n"
            )
            % nodemod.short(currentnode),
            component="commitcloud",
        )
    return 0


@perftrace.tracefunc("Apply Cloud Changes")
def _applycloudchanges(repo, remotepath, lastsyncstate, cloudrefs, maxage, state, tr):
    # Pull all the new heads and any bookmark hashes we don't have. We need to
    # filter cloudrefs before pull as pull doesn't check if a rev is present
    # locally.
    newheads = [
        nodemod.hex(n)
        for n in repo.changelog.filternodes(
            [nodemod.bin(h) for h in cloudrefs.heads], inverse=True
        )
    ]
    assert newheads == newheads
    if maxage is not None and maxage >= 0:
        mindate = time.time() - maxage * 86400
        omittedheads = [
            head
            for head in newheads
            if head in cloudrefs.headdates and cloudrefs.headdates[head] < mindate
        ]
        if omittedheads:
            omittedheadslen = len(omittedheads)
            repo.ui.status(
                _n(
                    "omitting %d head that is older than %d days:\n",
                    "omitting %d heads that are older than %d days:\n",
                    omittedheadslen,
                )
                % (omittedheadslen, maxage)
            )
            counter = 0
            for head in reversed(omittedheads):
                if counter == _maxomittedheadsoutput:
                    remaining = len(omittedheads) - counter
                    repo.ui.status(
                        _n("  and %d older head\n", "  and %d older heads\n", remaining)
                        % remaining
                    )
                    break
                headdatestr = util.datestr(util.makedate(cloudrefs.headdates[head]))
                repo.ui.status(_("  %s from %s\n") % (head[:12], headdatestr))
                counter = counter + 1

        omittedheads = set(omittedheads)
        newheads = [head for head in newheads if head not in omittedheads]
    else:
        omittedheads = set()
    omittedbookmarks = []
    omittedremotebookmarks = []

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

    remotebookmarknewnodes = set()
    remotebookmarkupdates = {}
    if _isremotebookmarkssyncenabled(repo.ui):
        (remotebookmarkupdates, remotebookmarknewnodes) = _processremotebookmarks(
            repo, cloudrefs.remotebookmarks, lastsyncstate
        )

    if remotebookmarknewnodes or newheads:
        # Partition the heads into groups we can pull together.
        headgroups = _partitionheads(repo.ui, list(remotebookmarknewnodes) + newheads)
        _pullheadgroups(repo, remotepath, headgroups)

    omittedbookmarks.extend(
        _mergebookmarks(
            repo, tr, cloudrefs.bookmarks, lastsyncstate, omittedheads, maxage
        )
    )

    newremotebookmarks = {}
    if _isremotebookmarkssyncenabled(repo.ui):
        omittedremotebookmarks = _updateremotebookmarks(repo, tr, remotebookmarkupdates)
        newremotebookmarks = cloudrefs.remotebookmarks

    if newvisibleheads is not None:
        visibility.setvisibleheads(repo, [nodemod.bin(n) for n in newvisibleheads])

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

    # For a new join or a workspace switch, record cloudrefs.heads as backed up, not only the pulled commits that are missing locally.
    if lastsyncstate.version == 0:
        state.update(
            [nodemod.bin(head) for head in cloudrefs.heads if head in repo], tr
        )

    lastsyncstate.update(
        tr,
        newversion=cloudrefs.version,
        newheads=cloudrefs.heads,
        newbookmarks=cloudrefs.bookmarks,
        newremotebookmarks=newremotebookmarks,
        newmaxage=maxage,
        newomittedheads=list(omittedheads),
        newomittedbookmarks=omittedbookmarks,
        newomittedremotebookmarks=omittedremotebookmarks,
    )

    # Also update backup state.  These new heads are already backed up,
    # otherwise the server wouldn't have told us about them.
    state.update([nodemod.bin(head) for head in newheads], tr)


def _pullheadgroups(repo, remotepath, headgroups):
    backuplock.progresspulling(
        repo, [nodemod.bin(node) for newheads in headgroups for node in newheads]
    )
    with progress.bar(
        repo.ui, _("pulling from commit cloud"), total=len(headgroups)
    ) as prog:
        for index, headgroup in enumerate(headgroups):
            headgroupstr = " ".join([head[:12] for head in headgroup])
            url = repo.ui.paths.getpath(remotepath).url
            repo.ui.status(_("pulling %s from %s\n") % (headgroupstr, url))
            prog.value = (index, headgroupstr)
            with repo.ui.timesection("commitcloud_sync_pull"):
                repo.pull(
                    remotepath,
                    headnodes=[nodemod.bin(hexnode) for hexnode in headgroup],
                    quiet=False,
                )
            repo.connectionpool.close()


def _partitionheads(ui, heads):
    sizelimit = int(ui.config("commitcloud", "pullsizelimit"))
    it = iter(heads)
    return list(iter(lambda: tuple(itertools.islice(it, sizelimit)), ()))


def _processremotebookmarks(repo, cloudremotebooks, lastsyncstate):
    """calculate new state between the cloud remote bookmarks and the local
    remote bookmarks

    Performs a 3-way diff between the last sync remote bookmark state, new cloud
    state and local remote bookmarks.

    Returns (updates, newnodes) where:
    - updates is a dict {remotebookmark: newnode} representing the updates
      to the remote bookmarks
    - newnodes is a set of nodes that are not in the repository and must be pulled
    """

    def usecloudnode(cloudnode, localnode):
        """returns True if cloudnode should be a new state for the remote bookmark

        Both cloudnode and localnode are public commits."""
        unfi = repo
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
    omittedremotebookmarks = set(lastsyncstate.omittedremotebookmarks)
    allremotenames = set(localremotebooks.keys())
    allremotenames.update(cloudremotebooks.keys())
    allremotenames.update(omittedremotebookmarks)

    updates = {}
    for remotename in allremotenames:
        cloudnode = cloudremotebooks.get(remotename, None)
        localnode = localremotebooks.get(remotename, None)
        oldcloudnode = oldcloudremotebooks.get(remotename, None)
        if localnode is None and remotename in omittedremotebookmarks:
            localnode = oldcloudnode

        if cloudnode != oldcloudnode and localnode != oldcloudnode:
            # Both cloud and local remote bookmark have changed.
            if cloudnode == localnode:
                # They have changed to the same thing
                updates[remotename] = localnode
            elif cloudnode and localnode:
                # They have changed to different things - break the tie by
                # seeing which is more up-to-date.
                if usecloudnode(cloudnode, localnode):
                    updates[remotename] = cloudnode
                else:
                    updates[remotename] = localnode
            elif oldcloudnode and not cloudnode:
                # The cloud remotebookmark was removed
                updates[remotename] = nodemod.nullhex
            elif localnode:
                # Use the local node
                updates[remotename] = localnode
        elif cloudnode and cloudnode != oldcloudnode:
            # The cloud node has updated, use the new version
            updates[remotename] = cloudnode
        elif oldcloudnode and not cloudnode:
            # The cloud remotebookmark was removed
            updates[remotename] = nodemod.nullhex
        elif localnode:
            # Use the local node
            updates[remotename] = localnode

    def ispublic(name):
        remote, name = bookmarks.splitremotename(name)
        return not repo._scratchbranchmatcher.match(name)

    unfi = repo
    newnodes = set(
        node
        for name, node in pycompat.iteritems(updates)
        if node != nodemod.nullhex and node not in unfi and ispublic(name)
    )
    return (updates, newnodes)


def _updateremotebookmarks(repo, tr, updates):
    """updates the remote bookmarks to point their new nodes"""
    oldremotebookmarks = _getremotebookmarks(repo)
    protectednames = _getprotectedremotebookmarks(repo)
    newremotebookmarks = {}
    omittedremotebookmarks = []
    unfi = repo

    # Filter out any deletions of default names.  These are protected and shouldn't
    # be deleted if this is the default remote
    for remotename, node in pycompat.iteritems(updates):
        remote, name = bookmarks.splitremotename(remotename)
        if node == nodemod.nullhex and remotename in protectednames:
            newremotebookmarks[remotename] = oldremotebookmarks.get(
                remotename, nodemod.nullhex
            )
        elif node != nodemod.nullhex and node not in unfi:
            omittedremotebookmarks.append(name)
            newremotebookmarks[remotename] = nodemod.nullhex
        else:
            newremotebookmarks[remotename] = node
    repo._remotenames.applychanges({"bookmarks": newremotebookmarks})

    return omittedremotebookmarks


def _forcesyncremotebookmarks(repo, cloudrefs, lastsyncstate, remotepath, tr):
    cloudremotebookmarks = cloudrefs.remotebookmarks or {}
    (updates, newnodes) = _processremotebookmarks(
        repo, cloudremotebookmarks, lastsyncstate
    )
    if newnodes:
        _pullheadgroups(repo, remotepath, _partitionheads(repo.ui, newnodes))
    omittedremotebookmarks = _updateremotebookmarks(repo, tr, updates)

    # We have now synced the repo to the cloud version.  Store this.
    lastsyncstate.update(
        tr,
        newremotebookmarks=cloudremotebookmarks,
        newomittedremotebookmarks=omittedremotebookmarks,
    )


def _mergebookmarks(repo, tr, cloudbookmarks, lastsyncstate, omittedheads, maxage):
    """merge any changes to the cloud bookmarks with any changes to local ones

    This performs a 3-way diff between the old cloud bookmark state, the new
    cloud bookmark state, and the local bookmark state.  If either local or
    cloud bookmarks have been modified, propagate those changes to the other.
    If both have been modified then fork the bookmark by renaming the local one
    and accepting the cloud bookmark's new value.

    Some of the bookmark changes may not be possible to apply, as the bookmarked
    commit has been omitted locally.  In that case the bookmark is omitted.

    Some of the bookmarks unknown locally are for the public commits,
    we apply them according to the requested maxage. If they are too old, they are omitted.

    Returns a list of the omitted bookmark names.
    """
    unfi = repo
    localbookmarks = _getbookmarks(repo)
    omittedbookmarks = set(lastsyncstate.omittedbookmarks)
    changes = []
    allnames = set(list(localbookmarks.keys()) + list(cloudbookmarks.keys()))
    newnames = set()
    mindate = (time.time() - maxage * 86400) if maxage is not None else 0
    oldbookmarks = {}
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
                        ctx = unfi[cloudnode]
                        # The cloud bookmark is for a public commit but older than the requested age.
                        if (
                            localnode is None
                            and not ctx.mutable()
                            and ctx.date()[0] < mindate
                        ):
                            oldbookmarks[name] = cloudnode
                            omittedbookmarks.add(name)
                        else:
                            # The commit is available locally, so update the
                            # bookmark.
                            changes.append((name, nodemod.bin(cloudnode)))
                            omittedbookmarks.discard(name)
                    else:
                        # The commit is not available locally.  Omit it.
                        if cloudnode not in omittedheads:
                            repo.ui.warn(
                                _("%s not found, omitting %s bookmark\n")
                                % (cloudnode[:12], name)
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

    if oldbookmarks:
        counter = 0
        for name, node in oldbookmarks.items():
            if counter == _maxomittedbookmarksoutput:
                remaining = len(oldbookmarks) - counter
                repo.ui.status(
                    _n(
                        "  and %d more old bookmark\n",
                        "  and %d more old bookmarks\n",
                        remaining,
                    )
                    % remaining
                )
                break
            repo.ui.status(
                _(
                    "%s is older than %d days, omitting %s bookmark\n",
                )
                % (node[:12], maxage, name)
            )
            counter = counter + 1

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


@perftrace.tracefunc("Check Omissions")
def _checkomissions(repo, remotepath, lastsyncstate, tr, maxage):
    """check omissions are still not available locally

    Check that the commits that have been deliberately omitted are still not
    available locally.  If they are now available (e.g. because the user pulled
    them manually), then remove the tracking of those heads being omitted, and
    restore any bookmarks that can now be restored.
    """
    unfi = repo
    lastomittedheads = set(lastsyncstate.omittedheads)
    lastomittedbookmarks = set(lastsyncstate.omittedbookmarks)
    lastomittedremotebookmarks = set(lastsyncstate.omittedremotebookmarks)
    omittedheads = set()
    omittedbookmarks = set()
    omittedremotebookmarks = set()
    changes = []
    remotechanges = {}
    mindate = (time.time() - maxage * 86400) if maxage is not None else 0
    foundheads = repo.changelog.filternodes(
        [nodemod.bin(n) for n in lastomittedheads], local=True
    )
    foundheads = {nodemod.hex(n) for n in foundheads}
    omittedheads = lastomittedheads - foundheads

    lastbookmarknodes = [
        lastsyncstate.bookmarks[name]
        for name in lastomittedbookmarks
        if name in lastsyncstate.bookmarks
    ]
    lastremotebookmarknodes = [
        lastsyncstate.remotebookmarks[name]
        for name in lastomittedremotebookmarks
        if name in lastsyncstate.remotebookmarks
    ]
    foundbookmarkslocalnodes = {
        nodemod.hex(n)
        for n in repo.changelog.filternodes(
            [nodemod.bin(n) for n in lastbookmarknodes + lastremotebookmarknodes],
            local=True,
        )
    }

    for name in lastomittedbookmarks:
        # bookmark might be removed from cloud workspace by someone else
        if name not in lastsyncstate.bookmarks:
            continue
        node = lastsyncstate.bookmarks[name]
        if node in foundbookmarkslocalnodes:
            if unfi[node].mutable() or (unfi[node].date()[0] >= mindate):
                changes.append((name, nodemod.bin(node)))
            else:
                omittedbookmarks.add(name)
        else:
            omittedbookmarks.add(name)

    for name in lastomittedremotebookmarks:
        # remotebookmark might be removed from cloud workspace by someone else
        if name not in lastsyncstate.remotebookmarks:
            continue
        node = lastsyncstate.remotebookmarks[name]
        if node in foundbookmarkslocalnodes:
            remotechanges[name] = node
        else:
            omittedremotebookmarks.add(name)
    if (
        omittedheads != lastomittedheads
        or omittedbookmarks != lastomittedbookmarks
        or omittedremotebookmarks != lastomittedremotebookmarks
    ):
        lastsyncstate.update(
            tr,
            newomittedheads=list(omittedheads),
            newomittedbookmarks=list(omittedbookmarks),
            newomittedremotebookmarks=list(omittedremotebookmarks),
        )
    if changes or remotechanges:
        with repo.wlock(), repo.lock(), repo.transaction("cloudsync") as tr:
            if changes:
                repo._bookmarks.applychanges(repo, tr, changes)
            if remotechanges:
                remotebookmarks = _getremotebookmarks(repo)
                remotebookmarks.update(remotechanges)
                repo._remotenames.applychanges({"bookmarks": remotebookmarks})


@perftrace.tracefunc("Submit Local Changes")
def _submitlocalchanges(repo, reponame, workspacename, lastsyncstate, failed, serv, tr):
    localheads = _getheads(repo)
    localbookmarks = _getbookmarks(repo)
    localremotebookmarks = _getremotebookmarks(repo)

    # If any commits failed to back up, exclude them.  Revert any bookmark changes
    # that point to failed commits.
    if failed:
        localheads = [
            nodemod.hex(head)
            for head in repo.nodes("heads(draft() & ::%ls - %ld::)", localheads, failed)
        ]
        failedset = set(repo.nodes("draft() & %ld::", failed))
        for name, bookmarknode in list(localbookmarks.items()):
            if nodemod.bin(bookmarknode) in failedset:
                if name in lastsyncstate.bookmarks:
                    localbookmarks[name] = lastsyncstate.bookmarks[name]
                else:
                    del localbookmarks[name]

    # Work out what we should have synced locally (and haven't deliberately
    # omitted)
    omittedheads = set(lastsyncstate.omittedheads)
    omittedbookmarks = set(lastsyncstate.omittedbookmarks)
    omittedremotebookmarks = set(lastsyncstate.omittedremotebookmarks)
    localsyncedheads = [
        head for head in lastsyncstate.heads if head not in omittedheads
    ]
    localsyncedbookmarks = {
        name: node
        for name, node in lastsyncstate.bookmarks.items()
        if name not in omittedbookmarks
    }
    localsyncedremotebookmarks = {
        name: node
        for name, node in lastsyncstate.remotebookmarks.items()
        if name not in omittedremotebookmarks
    }

    remotebookmarkschanged = (
        _isremotebookmarkssyncenabled(repo.ui)
        and localremotebookmarks != localsyncedremotebookmarks
    )

    if (
        set(localheads) == set(localsyncedheads)
        and localbookmarks == localsyncedbookmarks
        and not remotebookmarkschanged
        and lastsyncstate.version != 0
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

    oldremotebookmarks = []
    newremotebookmarks = {}
    newomittedremotebookmarks = []
    if _isremotebookmarkssyncenabled(repo.ui):
        # do not need to submit local remote bookmarks if the feature is not enabled
        oldremotebookmarks = lastsyncstate.remotebookmarks.keys()
        newremotebookmarks = {
            name: localremotebookmarks.get(
                name, lastsyncstate.remotebookmarks.get(name)
            )
            for name in set(localremotebookmarks.keys()).union(
                lastsyncstate.omittedremotebookmarks
            )
        }
        newomittedremotebookmarks = list(
            set(newremotebookmarks.keys()).difference(localremotebookmarks.keys())
        )

    backuplock.progress(repo, "finishing synchronizing with '%s'" % workspacename)
    synced, cloudrefs = serv.updatereferences(
        reponame,
        workspacename,
        lastsyncstate.version,
        lastsyncstate.heads,
        newcloudheads,
        lastsyncstate.bookmarks.keys(),
        newcloudbookmarks,
        oldremotebookmarks,
        newremotebookmarks,
        clientinfo=service.makeclientinfo(repo, lastsyncstate),
        logopts={"metalogroot": hex(repo.svfs.metalog.root())},
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
            tr,
            newversion=cloudrefs.version,
            newheads=newcloudheads,
            newbookmarks=newcloudbookmarks,
            newremotebookmarks=newremotebookmarks,
            newomittedheads=newomittedheads,
            newomittedbookmarks=newomittedbookmarks,
            newomittedremotebookmarks=newomittedremotebookmarks,
        )

    return synced, cloudrefs
