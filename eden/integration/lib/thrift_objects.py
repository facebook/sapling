#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

from typing import Optional

from eden.fs.service.eden.thrift_types import (
    Added,
    ChangeNotification,
    CommitTransition,
    DirectoryRenamed,
    Dtype,
    LargeChangeNotification,
    LostChanges,
    LostChangesReason,
    Modified,
    Removed,
    Renamed,
    Replaced,
    SmallChangeNotification,
    StateChangeNotification,
    StateEntered,
    StateLeft,
)
from thrift.python.types import StructMeta


def getSmallChangeSafe(
    change: ChangeNotification,
) -> Optional[SmallChangeNotification]:
    if hasattr(change, "smallChange") and change.smallChange is not None:
        return change.smallChange
    return None


def getLargeChangeSafe(
    change: ChangeNotification,
) -> Optional[LargeChangeNotification]:
    if hasattr(change, "largeChange") and change.largeChange is not None:
        return change.largeChange
    return None


def buildSmallChange(
    changeType: StructMeta,
    fileType: Dtype,
    path: Optional[bytes] = None,
    from_path: Optional[bytes] = None,
    to_path: Optional[bytes] = None,
) -> ChangeNotification:
    if changeType is Added:
        assert path
        return ChangeNotification(
            smallChange=SmallChangeNotification(
                added=Added(fileType=fileType, path=path)
            )
        )
    elif changeType is Modified:
        assert path
        return ChangeNotification(
            smallChange=SmallChangeNotification(
                modified=Modified(fileType=fileType, path=path)
            )
        )
    elif changeType is Renamed:
        assert from_path
        assert to_path
        return ChangeNotification(
            smallChange=SmallChangeNotification(
                renamed=Renamed(
                    fileType=fileType,
                    from_=from_path,
                    to=to_path,
                )
            )
        )
    elif changeType is Replaced:
        assert from_path
        assert to_path
        return ChangeNotification(
            smallChange=SmallChangeNotification(
                replaced=Replaced(
                    fileType=fileType,
                    from_=from_path,
                    to=to_path,
                )
            )
        )

    elif changeType is Removed:
        assert path
        return ChangeNotification(
            smallChange=SmallChangeNotification(
                removed=Removed(fileType=fileType, path=path)
            )
        )
    return ChangeNotification()


def buildLargeChange(
    changeType: StructMeta,
    from_bytes: Optional[bytes] = None,
    to_bytes: Optional[bytes] = None,
    lost_change_reason: Optional[LostChangesReason] = None,
) -> ChangeNotification:
    if changeType is DirectoryRenamed:
        return ChangeNotification(
            largeChange=LargeChangeNotification(
                directoryRenamed=DirectoryRenamed(from_=from_bytes, to=to_bytes)
            )
        )
    elif changeType is CommitTransition:
        return ChangeNotification(
            largeChange=LargeChangeNotification(
                commitTransition=CommitTransition(from_=from_bytes, to=to_bytes)
            )
        )
    elif changeType is LostChanges:
        return ChangeNotification(
            largeChange=LargeChangeNotification(
                lostChanges=LostChanges(reason=lost_change_reason)
            )
        )
    return ChangeNotification()


def buildStateChange(changeType: StructMeta, name: str) -> ChangeNotification:
    if changeType is StateEntered:
        return ChangeNotification(
            stateChange=StateChangeNotification(stateEntered=StateEntered(name=name))
        )
    elif changeType is StateLeft:
        return ChangeNotification(
            stateChange=StateChangeNotification(stateLeft=StateLeft(name=name))
        )
    return ChangeNotification()
