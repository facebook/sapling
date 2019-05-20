# Copyright 2017-present Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from edenscm.mercurial import node as nodemod, smartset
from edenscm.mercurial.i18n import _, _n

from . import backupstate, commitcloudutil, dependencies


def backup(repo, revs=None, dest=None, **opts):
    """backs up the given revisions to commit cloud

    Returns (backedup, failed), where "backedup" is a revset of the commits that
    were backed up, and "failed" is a revset of the commits that could not be
    backed up.
    """
    path = commitcloudutil.getremotepath(repo, dest)
    state = backupstate.BackupState(repo, path)
    unfi = repo.unfiltered()

    if revs is None:
        # No revs specified.  Back up all visible commits that are not already
        # backed up.
        heads = unfi.revs("heads(draft() - hidden() - (draft() & ::%ln))", state.heads)
    else:
        # Some revs were specified.  Back up all of those commits that are not
        # already backed up.
        heads = unfi.revs(
            "heads((draft() & ::%ld) - (draft() & ::%ln))", revs, state.heads
        )

    if not heads:
        return (smartset.baseset(), smartset.baseset())

    def getconnection():
        return repo.connectionpool.get(path, opts)

    # Check if any of the heads are already available on the server.
    headnodes = list(repo.nodes("%ld", heads))
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
        state.update(remoteheadnodes)

    heads = unfi.revs("%ld - %ln", heads, remoteheadnodes)

    if not heads:
        return (smartset.baseset(), smartset.baseset())

    # Filter out any commits that have been marked as bad.
    badnodes = repo.ui.configlist("infinitepushbackup", "dontbackupnodes", [])
    if badnodes:
        badnodes = [node for node in badnodes if node in unfi]
        # The nodes we can't back up are the bad nodes and their descendants,
        # minus any commits that we know are already backed up anyway.
        badnodes = list(
            unfi.nodes(
                "(draft() & ::%ld) & (%ls::) - (draft() & ::%ln)",
                heads,
                badnodes,
                state.heads,
            )
        )
        if badnodes:
            repo.ui.warn(
                _("not backing up commits marked as bad: %s\n")
                % ", ".join([nodemod.hex(node) for node in badnodes])
            )
            heads = unfi.revs("heads((draft() & ::%ld) - %ln)", heads, badnodes)

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
    newheads, failedheads = dependencies.infinitepush.pushbackupbundlestacks(
        repo.ui, unfi, getconnection, [nodemod.hex(n) for n in unfi.nodes("%ld", heads)]
    )

    # The commits that got backed up are all the ancestors of the new backup
    # heads, minus any commits that were already backed up at the start.
    backedup = unfi.revs("(draft() & ::%ls) - (draft() & ::%ln)", newheads, state.heads)
    # The commits that failed to get backed up are the ancestors of the failed
    # heads, except for commits that are also ancestors of a successfully backed
    # up head, or commits that were already known to be backed up.
    failed = unfi.revs(
        "(draft() & ::%ls) - (draft() & ::%ls) - (draft() & ::%ln)",
        failedheads,
        newheads,
        state.heads,
    )

    state.update(unfi.nodes("%ld", backedup))

    return backedup, failed
