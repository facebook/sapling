#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import binascii
from pathlib import Path
from typing import Callable, List, NamedTuple, Optional, Union

import facebook.eden.ttypes as eden_ttypes


class ResetParentsCommitsArgs(NamedTuple):
    mount: bytes
    parent1: bytes
    parent2: Optional[bytes]
    hg_root_manifest: Optional[bytes]


class FakeClient:
    def __init__(self) -> None:
        self._mounts = []
        self.set_parents_calls: List[ResetParentsCommitsArgs] = []

    def __enter__(self) -> "FakeClient":
        return self

    def __exit__(self, exc_type, exc_value, exc_traceback) -> None:
        pass

    def change_mount_state(
        self, path: Path, state: Optional[eden_ttypes.MountState]
    ) -> None:
        """This function allows tests to change the reported state of mounts."""
        path_bytes = bytes(path)
        for mount in self._mounts:
            if mount.mountPoint == path_bytes:
                mount.state = state
                return
        raise KeyError(f"no mount found at {path}")

    def listMounts(self):
        return self._mounts

    def resetParentCommits(
        self,
        mountPoint: bytes,
        parents: eden_ttypes.WorkingDirectoryParents,
        params: eden_ttypes.ResetParentCommitsParams,
    ) -> None:
        self.set_parents_calls.append(
            ResetParentsCommitsArgs(
                mount=mountPoint,
                parent1=parents.parent1,
                parent2=parents.parent2,
                hg_root_manifest=params.hgRootManifest,
            )
        )

    def debugInodeStatus(
        self,
        mountPoint: bytes,
        path: bytes,
        flags: int,
        sync: eden_ttypes.SyncBehavior,
    ) -> List[eden_ttypes.TreeInodeEntryDebugInfo]:
        return []

    def getSHA1(
        self, mountPoint: bytes, paths: List[bytes], sync: eden_ttypes.SyncBehavior
    ) -> List[eden_ttypes.SHA1Result]:
        return []
