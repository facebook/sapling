# Copyright 2018-2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import itertools
import re
import socket
import time

from edenscm.mercurial import (
    exchange,
    extensions,
    hg,
    hintutil,
    node as nodemod,
    obsolete,
    templatefilters,
    util,
    visibility,
)
from edenscm.mercurial.i18n import _

from . import (
    commitcloudcommon,
    commitcloudutil,
    dependencies,
    service,
    syncstate,
    workspace,
)


highlightstatus = commitcloudcommon.highlightstatus


def _getheads(repo):
    if visibility.enabled(repo):
        return [nodemod.hex(n) for n in visibility.heads(repo)]
    else:
        headsrevset = repo.set(
            "heads(draft() & ::((draft() & not obsolete()) + bookmark()))"
        )
        return [ctx.hex() for ctx in headsrevset]


def _getbookmarks(repo):
    return {n: nodemod.hex(v) for n, v in repo._bookmarks.items()}


def _backingupsyncprogress(repo, backingup):
    backingupmsg = (
        "backing up %s" % backingup[0][:12]
        if len(backingup) == 1
        else "backing up %d commits" % len(backingup)
    )
    commitcloudutil.writesyncprogress(repo, backingupmsg, backingup=backingup)


def docloudsync(ui, repo, cloudrefs=None, **opts):
    start = time.time()

    tokenlocator = commitcloudutil.TokenLocator(ui)
    reponame = commitcloudutil.getreponame(repo)
    workspacename = workspace.currentworkspace(repo)
    if workspacename is None:
        raise commitcloudcommon.WorkspaceError(ui, _("undefined workspace"))
    serv = service.get(ui, tokenlocator.token)
    highlightstatus(ui, _("synchronizing '%s' with '%s'\n") % (reponame, workspacename))
    commitcloudutil.writesyncprogress(
        repo, "starting synchronizing with '%s'" % workspacename
    )

    lastsyncstate = syncstate.SyncState(repo, workspacename)
    remotepath = commitcloudutil.getremotepath(repo, ui, None)

    # external services can run cloud sync and know the lasest version
    version = opts.get("workspace_version")
    if version and version.isdigit() and int(version) <= lastsyncstate.version:
        highlightstatus(ui, _("this version has been already synchronized\n"))
        return 0

    if opts.get("full"):
        maxage = None
    else:
        maxage = ui.configint("commitcloud", "max_sync_age", None)
    fetchversion = lastsyncstate.version

    # the remote backend for storing Commit Cloud commit have been changed
    # switching between Mercurial <-> Mononoke
    if lastsyncstate.remotepath and remotepath != lastsyncstate.remotepath:
        highlightstatus(
            ui,
            _(
                "commits storage have been switched\n"
                "             from: %s\n"
                "             to: %s\n"
            )
            % (lastsyncstate.remotepath, remotepath),
        )
        fetchversion = 0

    # cloudrefs are passed in cloud rejoin
    if cloudrefs is None:
        # if we are doing a full sync, or maxage has changed since the last
        # sync, use 0 as the last version to get a fresh copy of the full state.
        if maxage != lastsyncstate.maxage:
            fetchversion = 0
        cloudrefs = serv.getreferences(reponame, workspacename, fetchversion)

    def getconnection():
        return repo.connectionpool.get(remotepath, opts)

    # the remote backend for storing Commit Cloud commit have been changed
    if lastsyncstate.remotepath and remotepath != lastsyncstate.remotepath:
        commitcloudutil.writesyncprogress(
            repo, "verifying backed up heads at '%s'" % remotepath
        )
        # make sure cloudrefs.heads have been backed up at this remote path
        verifybackedupheads(
            repo, remotepath, lastsyncstate.remotepath, getconnection, cloudrefs.heads
        )
        # if verification succeeded, update remote path in the local state and go on
        lastsyncstate.updateremotepath(remotepath)

    synced = False
    pushfailures = set()
    prevsyncversion = lastsyncstate.version
    prevsyncheads = lastsyncstate.heads
    prevsyncbookmarks = lastsyncstate.bookmarks
    prevsynctime = lastsyncstate.lastupdatetime or 0
    while not synced:
        if cloudrefs.version != fetchversion:
            _applycloudchanges(ui, repo, remotepath, lastsyncstate, cloudrefs, maxage)

        # Check if any omissions are now included in the repo
        _checkomissions(ui, repo, remotepath, lastsyncstate)

        localheads = _getheads(repo)
        localbookmarks = _getbookmarks(repo)
        obsmarkers = commitcloudutil.getsyncingobsmarkers(repo)

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

        if not obsmarkers:
            # If the heads have changed, and we don't have any obsmakers to
            # send, then it's possible we have some obsoleted versions of
            # commits that are visible in the cloud workspace that need to
            # be revived.
            cloudvisibleonly = list(
                repo.unfiltered().set("draft() & ::%ls & hidden()", localsyncedheads)
            )
            repo._commitcloudskippendingobsmarkers = True
            obsolete.revive(cloudvisibleonly)
            repo._commitcloudskippendingobsmarkers = False
            localheads = _getheads(repo)

        if (
            set(localheads) == set(localsyncedheads)
            and localbookmarks == localsyncedbookmarks
            and lastsyncstate.version != 0
            and not obsmarkers
        ):
            synced = True

        if not synced:
            # The local repo has changed.  We must send these changes to the
            # cloud.

            # Push commits that the server doesn't have.
            newheads = list(set(localheads) - set(lastsyncstate.heads))

            # If there are too many heads to backup,
            # it is faster to check with the server first
            backuplimitnocheck = ui.configint("commitcloud", "backuplimitnocheck")
            if len(newheads) > backuplimitnocheck:
                isbackedupremote = dependencies.infinitepush.isbackedupnodes(
                    getconnection, newheads
                )
                newheads = [
                    head for i, head in enumerate(newheads) if not isbackedupremote[i]
                ]

            # all pushed to the server except maybe obsmarkers
            allpushed = (not newheads) and (localbookmarks == localsyncedbookmarks)

            failedheads = []
            unfi = repo.unfiltered()
            if not allpushed:
                oldheads = list(
                    set(lastsyncstate.heads) - set(lastsyncstate.omittedheads)
                )
                backingup = [
                    nodemod.hex(n)
                    for n in unfi.nodes("draft() & ::%ls - ::%ls", newheads, oldheads)
                ]
                _backingupsyncprogress(repo, backingup)
                newheads, failedheads = dependencies.infinitepush.pushbackupbundlestacks(
                    ui, repo, getconnection, newheads
                )

            if failedheads:
                pushfailures |= set(failedheads)
                # Some heads failed to be pushed.  Work out what is actually
                # available on the server
                localheads = [
                    ctx.hex()
                    for ctx in unfi.set(
                        "heads((draft() & ::%ls) + (draft() & ::%ls & ::%ls))",
                        newheads,
                        localheads,
                        localsyncedheads,
                    )
                ]
                failedcommits = {
                    ctx.hex()
                    for ctx in unfi.set(
                        "(draft() & ::%ls) - (draft() & ::%ls) - (draft() & ::%ls)",
                        failedheads,
                        newheads,
                        localsyncedheads,
                    )
                }
                # Revert any bookmark updates that refer to failed commits to
                # the available commits.
                for name, bookmarknode in localbookmarks.items():
                    if bookmarknode in failedcommits:
                        if name in lastsyncstate.bookmarks:
                            localbookmarks[name] = lastsyncstate.bookmarks[name]
                        else:
                            del localbookmarks[name]

            # Update the infinitepush backup bookmarks to point to the new
            # local heads and bookmarks.  This must be done after all
            # referenced commits have been pushed to the server.
            if not allpushed:
                pushbackupbookmarks(
                    ui,
                    repo,
                    remotepath,
                    getconnection,
                    localheads,
                    localbookmarks,
                    **opts
                )

            # Work out the new cloud heads and bookmarks by merging in the
            # omitted items.  We need to preserve the ordering of the cloud
            # heads so that smartlogs generally match.
            newcloudheads = [
                head
                for head in lastsyncstate.heads
                if head in set(localheads) | set(lastsyncstate.omittedheads)
            ]
            newcloudheads.extend(
                [head for head in localheads if head not in set(newcloudheads)]
            )
            newcloudbookmarks = {
                name: localbookmarks.get(name, lastsyncstate.bookmarks.get(name))
                for name in set(localbookmarks.keys())
                | set(lastsyncstate.omittedbookmarks)
            }
            newomittedheads = list(set(newcloudheads) - set(localheads))
            newomittedbookmarks = list(
                set(newcloudbookmarks.keys()) - set(localbookmarks.keys())
            )

            if (
                prevsyncversion == lastsyncstate.version - 1
                and prevsyncheads == newcloudheads
                and prevsyncbookmarks == newcloudbookmarks
                and prevsynctime > time.time() - 60
            ):
                raise commitcloudcommon.SynchronizationError(
                    ui,
                    _(
                        "oscillating commit cloud workspace detected.\n"
                        "check for commits that are visible in one repo but hidden in another,\n"
                        "and hide or unhide those commits in all places."
                    ),
                )

            # Update the cloud heads, bookmarks and obsmarkers.
            commitcloudutil.writesyncprogress(
                repo, "finishing synchronizing with '%s'" % workspacename
            )
            synced, cloudrefs = serv.updatereferences(
                reponame,
                workspacename,
                lastsyncstate.version,
                lastsyncstate.heads,
                newcloudheads,
                lastsyncstate.bookmarks.keys(),
                newcloudbookmarks,
                obsmarkers,
            )
            if synced:
                lastsyncstate.update(
                    cloudrefs.version,
                    newcloudheads,
                    newcloudbookmarks,
                    newomittedheads,
                    newomittedbookmarks,
                    maxage,
                    remotepath,
                )
                if obsmarkers:
                    commitcloudutil.clearsyncingobsmarkers(repo)

    commitcloudutil.writesyncprogress(repo)
    if pushfailures:
        raise commitcloudcommon.SynchronizationError(
            ui, _("%d heads could not be pushed") % len(pushfailures)
        )
    highlightstatus(ui, _("commits synchronized\n"))
    # check that Scm Service is running and a subscription exists
    commitcloudutil.SubscriptionManager(repo).checksubscription()
    elapsed = time.time() - start
    ui.status(_("finished in %0.2f sec\n") % elapsed)


def maybeupdateworkingcopy(ui, repo, currentnode):
    if repo["."].node() != currentnode:
        return 0

    destination = finddestinationnode(repo, currentnode)

    if destination == currentnode:
        return 0

    if destination and destination in repo:
        highlightstatus(
            ui,
            _("current revision %s has been moved remotely to %s\n")
            % (nodemod.short(currentnode), nodemod.short(destination)),
        )
        if ui.configbool("commitcloud", "updateonmove"):
            if repo[destination].mutable():
                commitcloudutil.writesyncprogress(
                    repo,
                    "updating %s from %s to %s"
                    % (
                        repo.wvfs.base,
                        nodemod.short(currentnode),
                        nodemod.short(destination),
                    ),
                )
                return _update(ui, repo, destination)
        else:
            hintutil.trigger("commitcloud-update-on-move")
    else:
        highlightstatus(
            ui,
            _(
                "current revision %s has been replaced remotely "
                "with multiple revisions\n"
                "Please run `hg update` to go to the desired revision\n"
            )
            % nodemod.short(currentnode),
        )
    return 0


def verifybackedupheads(repo, remotepath, oldremotepath, getconnection, heads):
    if not heads:
        return

    backedupheadsremote = {
        head
        for head, backedup in zip(
            heads, dependencies.infinitepush.isbackedupnodes(getconnection, heads)
        )
        if backedup
    }

    notbackedupheads = set(heads) - backedupheadsremote
    notbackeduplocalheads = {head for head in notbackedupheads if head in repo}

    if notbackeduplocalheads:
        backingup = list(notbackeduplocalheads)
        _backingupsyncprogress(repo, backingup)
        repo.ui.status(_("pushing to %s\n") % remotepath)
        dependencies.infinitepush.pushbackupbundlestacks(
            repo.ui, repo, getconnection, backingup
        )
        recordbackup(repo.ui, repo, remotepath, backingup)

    if len(notbackedupheads) != len(notbackeduplocalheads):
        missingheads = list(notbackedupheads - notbackeduplocalheads)
        highlightstatus(repo.ui, _("some heads are missing at %s\n") % remotepath)
        commitcloudutil.writesyncprogress(repo, "pulling %s" % missingheads[0][:12])
        pullcmd, pullopts = commitcloudutil.getcommandandoptions("^pull")
        pullopts["rev"] = missingheads
        pullcmd(repo.ui, repo.unfiltered(), oldremotepath, **pullopts)
        backingup = list(missingheads)
        _backingupsyncprogress(repo, backingup)
        repo.ui.status(_("pushing to %s\n") % remotepath)
        dependencies.infinitepush.pushbackupbundlestacks(
            repo.ui, repo, getconnection, backingup
        )
        recordbackup(repo.ui, repo, remotepath, backingup)

    return 0


def finddestinationnode(repo, startnode):
    nodes = list(repo.nodes("successors(%n) - obsolete()", startnode))
    if len(nodes) == 0:
        return startnode
    elif len(nodes) == 1:
        return nodes[0]
    else:
        return None


def pushbackupbookmarks(
    ui, repo, remotepath, getconnection, localheads, localbookmarks, **opts
):
    """
    Push a backup bundle to the server that updates the infinitepush backup
    bookmarks.

    This keeps the old infinitepush backup bookmarks in sync, which means
    pullbackup still works for users using commit cloud sync.
    """
    # Build a dictionary of infinitepush bookmarks.  We delete
    # all bookmarks and replace them with the full set each time.
    if dependencies.infinitepushbackup is not None:
        infinitepushbookmarks = {}
        namingmgr = dependencies.infinitepushbackup.BackupBookmarkNamingManager(
            ui, repo, opts.get("user")
        )
        infinitepushbookmarks[namingmgr.getbackupheadprefix()] = ""
        infinitepushbookmarks[namingmgr.getbackupbookmarkprefix()] = ""
        for bookmark, hexnode in localbookmarks.items():
            name = namingmgr.getbackupbookmarkname(bookmark)
            infinitepushbookmarks[name] = hexnode
        for hexhead in localheads:
            name = namingmgr.getbackupheadname(hexhead)
            infinitepushbookmarks[name] = hexhead

        # Push a bundle containing the new bookmarks to the server.
        with getconnection() as conn:
            dependencies.infinitepush.pushbackupbundle(
                ui, repo, conn.peer, None, infinitepushbookmarks
            )

        # Update the infinitepush local state.
        dependencies.infinitepushbackup._writelocalbackupstate(
            repo.sharedvfs, remotepath, list(localheads), localbookmarks
        )


def recordbackup(ui, repo, remotepath, newheads):
    """Record that the given heads are already backed up."""
    if dependencies.infinitepushbackup is None:
        return

    backupstate = dependencies.infinitepushbackup._readlocalbackupstate(
        ui, repo, remotepath
    )
    backupheads = set(backupstate.heads) | set(newheads)
    dependencies.infinitepushbackup._writelocalbackupstate(
        repo.sharedvfs, remotepath, list(backupheads), backupstate.localbookmarks
    )


def _applycloudchanges(ui, repo, remotepath, lastsyncstate, cloudrefs, maxage=None):
    pullcmd, pullopts = commitcloudutil.getcommandandoptions("^pull")

    try:
        remotenames = extensions.find("remotenames")
    except KeyError:
        remotenames = None

    # Pull all the new heads and any bookmark hashes we don't have. We need to
    # filter cloudrefs before pull as pull does't check if a rev is present
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
                head for head in lastsyncstate.heads if head not in omittedheads
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

    if len(newheads) > 1:
        commitcloudutil.writesyncprogress(
            repo, "pulling %d new heads" % len(newheads), newheads=newheads
        )
    elif len(newheads) == 1:
        commitcloudutil.writesyncprogress(
            repo, "pulling %s" % nodemod.short(newheads[0]), newheads=newheads
        )

    if newheads:
        # Replace the exchange pullbookmarks function with one which updates the
        # user's synced bookmarks.  This also means we don't partially update a
        # subset of the remote bookmarks if they happen to be included in the
        # pull.
        def _pullbookmarks(orig, pullop):
            if "bookmarks" in pullop.stepsdone:
                return
            pullop.stepsdone.add("bookmarks")
            tr = pullop.gettransaction()
            omittedbookmarks.extend(
                _mergebookmarks(pullop.repo, tr, cloudrefs.bookmarks, lastsyncstate)
            )

        # Replace the exchange pullobsolete function with one which adds the
        # cloud obsmarkers to the repo and updates visibility to match the
        # cloud heads.
        def _pullobsolete(orig, pullop):
            if "obsmarkers" in pullop.stepsdone:
                return
            pullop.stepsdone.add("obsmarkers")
            tr = pullop.gettransaction()
            _mergeobsmarkers(pullop.repo, tr, cloudrefs.obsmarkers)
            if newvisibleheads is not None:
                visibility.setvisibleheads(
                    pullop.repo, [nodemod.bin(n) for n in newvisibleheads]
                )

        # Disable pulling of remotenames.
        def _pullremotenames(orig, repo, remote, bookmarks):
            pass

        pullopts["rev"] = newheads
        with extensions.wrappedfunction(
            exchange, "_pullobsolete", _pullobsolete
        ), extensions.wrappedfunction(
            exchange, "_pullbookmarks", _pullbookmarks
        ), extensions.wrappedfunction(
            remotenames, "pullremotenames", _pullremotenames
        ) if remotenames else util.nullcontextmanager():
            pullcmd(ui, repo, remotepath, **pullopts)
    else:
        with repo.wlock(), repo.lock(), repo.transaction("cloudsync") as tr:
            omittedbookmarks.extend(
                _mergebookmarks(repo, tr, cloudrefs.bookmarks, lastsyncstate)
            )
            _mergeobsmarkers(repo, tr, cloudrefs.obsmarkers)
            if newvisibleheads is not None:
                visibility.setvisibleheads(
                    repo, [nodemod.bin(n) for n in newvisibleheads]
                )

    # We have now synced the repo to the cloud version.  Store this.
    lastsyncstate.update(
        cloudrefs.version,
        cloudrefs.heads,
        cloudrefs.bookmarks,
        omittedheads,
        omittedbookmarks,
        maxage,
        remotepath,
    )

    # Also update infinitepush state.  These new heads are already backed up,
    # otherwise the server wouldn't have told us about them.
    recordbackup(ui, repo, remotepath, newheads)


def _checkomissions(ui, repo, remotepath, lastsyncstate):
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
            remotepath,
        )
    if changes:
        with repo.wlock(), repo.lock(), repo.transaction("cloudsync") as tr:
            repo._bookmarks.applychanges(repo, tr, changes)


def _update(ui, repo, destination):
    # update to new head with merging local uncommited changes
    ui.status(_("updating to %s\n") % nodemod.short(destination))
    updatecheck = "noconflict"
    return hg.updatetotally(ui, repo, destination, destination, updatecheck=updatecheck)


def _filterpushside(ui, repo, pushheads, localheads, lastsyncstateheads):
    """filter push side to include only the specified push heads to the delta"""

    # local - allowed - synced
    skipped = set(localheads) - set(pushheads) - set(lastsyncstateheads)
    if skipped:

        def firstline(hexnode):
            return templatefilters.firstline(repo[hexnode].description())[:50]

        skippedlist = "\n".join(
            ["    %s    %s" % (hexnode[:16], firstline(hexnode)) for hexnode in skipped]
        )
        highlightstatus(
            ui,
            _("push filter: list of unsynced local heads that will be skipped\n%s\n")
            % skippedlist,
        )

    return list(set(localheads) & (set(lastsyncstateheads) | set(pushheads)))


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


def _mergeobsmarkers(repo, tr, obsmarkers):
    tr._commitcloudskippendingobsmarkers = True
    repo.obsstore.add(tr, obsmarkers)


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
