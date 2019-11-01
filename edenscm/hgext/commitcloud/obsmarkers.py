# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

from edenscm.mercurial import lock as lockmod, obsolete


# Obsmarker syncing.
#
# To avoid lock interactions between transactions (which may add new obsmarkers)
# and sync (which wants to clear the obsmarkers), we use a two-stage process to
# sync obsmarkers.
#
# When a transaction completes, we append any new obsmarkers to the pending
# obsmarker file.  This is protected by the obsmarkers lock.
#
# When a sync operation starts, we transfer these obsmarkers to the syncing
# obsmarker file.  This transfer is also protected by the obsmarkers lock, but
# the transfer should be very quick as it's just moving a small amount of data.
#
# The sync process can upload the syncing obsmarkers at its leisure.  The
# syncing obsmarkers file is protected by the infinitepush backup lock.

_obsmarkerslockname = "commitcloudpendingobsmarkers.lock"
_obsmarkerslocktimeout = 2
_obsmarkerspending = "commitcloudpendingobsmarkers"
_obsmarkerssyncing = "commitcloudsyncingobsmarkers"


def addpendingobsmarkers(repo, markers):
    with lockmod.lock(repo.svfs, _obsmarkerslockname, timeout=_obsmarkerslocktimeout):
        with repo.svfs.open(_obsmarkerspending, "ab") as f:
            offset = f.tell()
            # offset == 0: new file - add the version header
            data = b"".join(
                obsolete.encodemarkers(markers, offset == 0, obsolete._fm1version)
            )
            f.write(data)


def getsyncingobsmarkers(repo):
    """Transfers any pending obsmarkers, and returns all syncing obsmarkers.

    The caller must hold the backup lock.
    """
    # Move any new obsmarkers from the pending file to the syncing file
    with lockmod.lock(repo.svfs, _obsmarkerslockname, timeout=_obsmarkerslocktimeout):
        if repo.svfs.exists(_obsmarkerspending):
            with repo.svfs.open(_obsmarkerspending) as f:
                _version, markers = obsolete._readmarkers(f.read())
            with repo.sharedvfs.open(_obsmarkerssyncing, "ab") as f:
                offset = f.tell()
                # offset == 0: new file - add the version header
                data = b"".join(
                    obsolete.encodemarkers(markers, offset == 0, obsolete._fm1version)
                )
                f.write(data)
            repo.svfs.unlink(_obsmarkerspending)

    # Load the syncing obsmarkers
    markers = []
    if repo.sharedvfs.exists(_obsmarkerssyncing):
        with repo.sharedvfs.open(_obsmarkerssyncing) as f:
            _version, markers = obsolete._readmarkers(f.read())
    return markers


def clearsyncingobsmarkers(repo):
    """Clears all syncing obsmarkers.  The caller must hold the backup lock."""
    repo.sharedvfs.tryunlink(_obsmarkerssyncing)
