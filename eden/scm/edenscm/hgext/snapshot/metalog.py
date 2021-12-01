# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

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
    if snapshot is not None:
        ml.set(LATESTSNAPSHOT, snapshot)
    if bubble is not None:
        ml.set(LATESTBUBBLE, str(bubble).encode("ascii"))
    ml.commit("Save latest bubble/snapshot")
