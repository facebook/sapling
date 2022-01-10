# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from edenscm.mercurial import error

LATESTBUBBLE = "latestbubble"
LATESTSNAPSHOT = "latestsnapshot"


def fetchlatestbubble(ml):
    data = ml.get(LATESTBUBBLE)
    if data is not None:
        try:
            return int(data.decode("ascii"))
        except Exception:
            return None


def fetchlatestsnapshot(ml):
    return ml.get(LATESTSNAPSHOT)


def storelatest(ml, snapshot, bubble):
    try:
        if snapshot is not None:
            ml.set(LATESTSNAPSHOT, snapshot)
        if bubble is not None:
            ml.set(LATESTBUBBLE, str(bubble).encode("ascii"))
        ml.commit("Save latest bubble/snapshot")
    except error.MetaLogError:
        # Writing bubbles to metalog is of secondary importance, we don't want
        # to fail everything. Ideally we want to overwrite the metalog entries,
        # but that's not easy right now.
        pass
