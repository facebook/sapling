#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

from typing import Optional

from facebook.eden.ttypes import (
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
)


def getSmallChangeSafe(
    change: ChangeNotification,
) -> Optional[SmallChangeNotification]:
    if change.getType() == ChangeNotification.SMALLCHANGE:
        return change.get_smallChange()
    return None


def getLargeChangeSafe(
    change: ChangeNotification,
) -> Optional[LargeChangeNotification]:
    if change.getType() == ChangeNotification.LARGECHANGE:
        return change.get_largeChange()
    return None


def buildSmallChange(
    changeType: int,
    fileType: Dtype,
    path: Optional[bytes] = None,
    from_path: Optional[bytes] = None,
    to_path: Optional[bytes] = None,
) -> ChangeNotification:
    if changeType == SmallChangeNotification.ADDED:
        assert path
        return ChangeNotification(
            SmallChangeNotification(added=Added(fileType=fileType, path=path))
        )
    elif changeType == SmallChangeNotification.MODIFIED:
        assert path
        return ChangeNotification(
            SmallChangeNotification(modified=Modified(fileType=fileType, path=path))
        )
    elif changeType == SmallChangeNotification.RENAMED:
        assert from_path
        assert to_path
        return ChangeNotification(
            smallChange=SmallChangeNotification(
                renamed=Renamed(
                    fileType=fileType,
                    from_PY_RESERVED_KEYWORD=from_path,
                    to=to_path,
                )
            )
        )
    elif changeType == SmallChangeNotification.REPLACED:
        assert from_path
        assert to_path
        return ChangeNotification(
            smallChange=SmallChangeNotification(
                replaced=Replaced(
                    fileType=Dtype.REGULAR,
                    from_PY_RESERVED_KEYWORD=from_path,
                    to=to_path,
                )
            )
        )

    elif changeType == SmallChangeNotification.REMOVED:
        assert path
        return ChangeNotification(
            SmallChangeNotification(removed=Removed(fileType=fileType, path=path))
        )
    return ChangeNotification()


def buildLargeChange(
    changeType: int,
    from_bytes: Optional[bytes] = None,
    to_bytes: Optional[bytes] = None,
    lost_change_reason: Optional[LostChangesReason] = None,
) -> ChangeNotification:
    if changeType == LargeChangeNotification.DIRECTORYRENAMED:
        return ChangeNotification(
            largeChange=LargeChangeNotification(
                directoryRenamed=DirectoryRenamed(
                    from_PY_RESERVED_KEYWORD=from_bytes, to=to_bytes
                )
            )
        )
    elif changeType == LargeChangeNotification.COMMITTRANSITION:
        return ChangeNotification(
            largeChange=LargeChangeNotification(
                commitTransition=CommitTransition(
                    from_PY_RESERVED_KEYWORD=from_bytes, to=to_bytes
                )
            )
        )
    elif changeType == LargeChangeNotification.LOSTCHANGES:
        return ChangeNotification(
            largeChange=LargeChangeNotification(
                lostChanges=LostChanges(reason=lost_change_reason)
            )
        )
    return ChangeNotification()
