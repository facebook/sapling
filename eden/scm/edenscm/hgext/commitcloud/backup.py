# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

from edenscm.mercurial import node as nodemod, perftrace, smartset
from edenscm.mercurial.i18n import _, _n

from . import backupbookmarks, backuplock, backupstate, dependencies, util as ccutil


def backup(repo, revs, connect_opts=None, dest=None, backupsnapshots=False):
    remotepath = ccutil.getremotepath(repo, dest)
    getconnection = lambda: repo.connectionpool.get(remotepath, connect_opts)

    with backuplock.lock(repo):
        # Load the backup state under the repo lock to ensure a consistent view.
        with repo.lock():
            state = backupstate.BackupState(repo, remotepath)
        backedup, failed = _backup(
            repo,
            state,
            remotepath,
            getconnection,
            revs,
            backupsnapshots=backupsnapshots,
        )

        if revs is None:
            # For a full backup, also update the backup bookmarks.
            backupbookmarks.pushbackupbookmarks(repo, remotepath, getconnection, state)

    return backedup, failed


@perftrace.tracefunc("Backup Draft Commits to Commit Cloud")
def _backup(
    repo, backupstate, remotepath, getconnection, revs=None, backupsnapshots=False
):
    """backs up the given revisions to commit cloud

    Returns (backedup, failed), where "backedup" is a revset of the commits that
    were backed up, and "failed" is a revset of the commits that could not be
    backed up.
    """
    unfi = repo.unfiltered()

    if revs is None:
        # No revs specified.  Back up all visible commits that are not already
        # backed up. Also back up all the snapshots if needed.
        snapshotcond = " + snapshot()" if backupsnapshots else ""
        revset = (
            "heads(not public() - hidden()%s - (not public() & ::%%ln))" % snapshotcond
        )
        heads = unfi.revs(revset, backupstate.heads)
    else:
        # Some revs were specified.  Back up all of those commits that are not
        # already backed up.
        heads = unfi.revs(
            "heads((not public() & ::%ld) - (not public() & ::%ln))",
            revs,
            backupstate.heads,
        )

    if not heads:
        return smartset.baseset(), smartset.baseset()

    # Check if any of the heads are already available on the server.
    headnodes = list(unfi.nodes("%ld", heads))
    remoteheadnodes = {
        head
        for head, backedup in zip(
            headnodes,
            dependencies.infinitepush.isbackedupnodes(
                getconnection, [nodemod.hex(n) for n in headnodes]
            ),
        )
        if backedup
    }
    if remoteheadnodes:
        backupstate.update(remoteheadnodes)

    heads = unfi.revs("%ld - %ln", heads, remoteheadnodes)

    if not heads:
        return smartset.baseset(), smartset.baseset()

    # Filter out any commits that have been marked as bad.
    badnodes = repo.ui.configlist("infinitepushbackup", "dontbackupnodes", [])
    if badnodes:
        badnodes = [node for node in badnodes if node in unfi]
        # The nodes we can't back up are the bad nodes and their descendants,
        # minus any commits that we know are already backed up anyway.
        badnodes = list(
            unfi.nodes(
                "(not public() & ::%ld) & (%ls::) - (not public() & ::%ln)",
                heads,
                badnodes,
                backupstate.heads,
            )
        )
        if badnodes:
            repo.ui.warn(
                _("not backing up commits marked as bad: %s\n")
                % ", ".join([nodemod.hex(node) for node in badnodes])
            )
            heads = unfi.revs("heads((not public() & ::%ld) - %ln)", heads, badnodes)

    # Limit the number of heads we backup in a single operation.
    backuplimit = repo.ui.configint("infinitepushbackup", "maxheadstobackup")
    if backuplimit is not None and backuplimit >= 0:
        if len(heads) > backuplimit:
            repo.ui.status(
                _n(
                    "backing up only the most recent %d head\n",
                    "backing up only the most recent %d heads\n",
                    backuplimit,
                )
                % backuplimit
            )
            heads = sorted(heads, reverse=True)[:backuplimit]

    # Back up the new heads.
    backingup = unfi.nodes(
        "(not public() & ::%ld) - (not public() & ::%ln)", heads, backupstate.heads
    )
    backuplock.progressbackingup(repo, list(backingup))
    with perftrace.trace("Push Backup Bundles"):
        newheads, failedheads = dependencies.infinitepush.pushbackupbundlestacks(
            repo.ui,
            unfi,
            getconnection,
            [nodemod.hex(n) for n in unfi.nodes("%ld", heads)],
        )

    # The commits that got backed up are all the ancestors of the new backup
    # heads, minus any commits that were already backed up at the start.
    backedup = unfi.revs(
        "(not public() & ::%ls) - (not public() & ::%ln)", newheads, backupstate.heads
    )
    # The commits that failed to get backed up are the ancestors of the failed
    # heads, except for commits that are also ancestors of a successfully backed
    # up head, or commits that were already known to be backed up.
    failed = unfi.revs(
        "(not public() & ::%ls) - (not public() & ::%ls) - (not public() & ::%ln)",
        failedheads,
        newheads,
        backupstate.heads,
    )

    backupstate.update(unfi.nodes("%ld", backedup))

    return backedup, failed
