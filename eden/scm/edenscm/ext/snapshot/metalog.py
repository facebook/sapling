# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from edenscm import error, perftrace

LATESTBUBBLE = "latestbubble"
LATESTSNAPSHOT = "latestsnapshot"


@perftrace.tracefunc("Fetch latest bubble")
def fetchlatestbubble(ml):
    data = ml.get(LATESTBUBBLE)
    if data is not None:
        try:
            return int(data.decode("ascii"))
        except Exception:
            return None


@perftrace.tracefunc("Fetch latest snapshot")
def fetchlatestsnapshot(ml):
    return ml.get(LATESTSNAPSHOT)


@perftrace.tracefunc("Snapshot metalog store")
def storelatest(repo, snapshot, bubble):
    """call this inside repo.transaction() to write changes to disk"""
    assert repo.currenttransaction()
    ml = repo.metalog()
    if snapshot is not None:
        ml.set(LATESTSNAPSHOT, snapshot)
    if bubble is not None:
        ml.set(LATESTBUBBLE, str(bubble).encode("ascii"))
