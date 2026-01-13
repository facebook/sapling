#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict


import os
from collections import defaultdict
from pathlib import Path
from typing import Dict, List, NamedTuple, Optional

# TODO(T250577897): Migrate code in util.py to use modern thrift-python types
import facebook.eden.ttypes as py_deprecated_eden_ttypes
from eden.fs.service.eden.thrift_types import (
    DebugInvalidateRequest,
    DebugInvalidateResponse,
    GetCurrentSnapshotInfoRequest,
    GetCurrentSnapshotInfoResponse,
    GetScmStatusParams,
    GetScmStatusResult,
    GetStatInfoParams,
    InternalStats,
    MatchFileSystemRequest,
    MatchFileSystemResponse,
    MountInfo,
    MountInodeInfo,
    MountState,
    ResetParentCommitsParams,
    RootIdOptions,
    ScmStatus,
    SHA1Result,
    SyncBehavior,
    TreeInodeEntryDebugInfo,
    WorkingDirectoryParents,
)

from .fake_mount_table import FakeMountTable


class ResetParentsCommitsArgs(NamedTuple):
    mount: bytes
    parent1: bytes
    parent2: Optional[bytes]
    hg_root_manifest: Optional[bytes]
    rootIdOptions: Optional[RootIdOptions]


class FakeClient:
    def __init__(self, eden_dir: Path, mount_table: FakeMountTable) -> None:
        self._eden_dir = eden_dir
        self._mount_table = mount_table
        self.set_parents_calls: List[ResetParentsCommitsArgs] = []

        # pyre won't infer the Optional type if we express this as a lambda.
        def _get_default_mount_state() -> Optional[MountState]:
            return MountState.RUNNING

        self._path_mount_state: Dict[bytes, Optional[MountState]] = defaultdict(
            _get_default_mount_state
        )

        self._path_mount_inode_info: Dict[bytes, MountInodeInfo] = defaultdict(
            lambda: MountInodeInfo(
                unloadedInodeCount=1, loadedFileCount=2, loadedTreeCount=3
            )
        )

        self._counter_values: Dict[str, int] = defaultdict(int)

    def __enter__(self) -> "FakeClient":
        return self

    # pyre-fixme[2]: Parameter must be annotated.
    def __exit__(self, exc_type, exc_value, exc_traceback) -> None:
        pass

    def change_mount_state(self, path: Path, state: Optional[MountState]) -> None:
        """This function allows tests to change the reported state of mounts."""
        self._path_mount_state[os.fsencode(path)] = state

    def set_mount_inode_info(
        self, path: Path, mount_inode_info: MountInodeInfo
    ) -> None:
        self._path_mount_inode_info[os.fsencode(path)] = mount_inode_info

    def set_counter_value(self, counter: str, value: int) -> None:
        self._counter_values[counter] = value

    def listMounts(self) -> List[py_deprecated_eden_ttypes.MountInfo]:
        result = []
        for mount in self._mount_table.mounts:
            mount_path = Path(os.fsdecode(mount.mount_point))
            client_name = mount_path.parts[-1]
            client_path = self._eden_dir / "clients" / client_name

            # Handle both None and actual mount states to preserve original behavior
            mount_state = self._path_mount_state[mount.mount_point]

            if mount_state is None:
                # For None states (old edenfs), create py-deprecated MountInfo directly
                # to preserve exact original behavior
                thrift_mount_info = py_deprecated_eden_ttypes.MountInfo(
                    mountPoint=mount.mount_point,
                    edenClientPath=os.fsencode(client_path),
                    state=None,
                )
            else:
                # For actual mount states, use thrift-python MountState and convert
                python_mount_info = MountInfo(
                    mountPoint=mount.mount_point,
                    edenClientPath=os.fsencode(client_path),
                    state=mount_state,
                )
                thrift_mount_info = python_mount_info._to_py_deprecated()

            result.append(thrift_mount_info)

        return result

    def resetParentCommits(
        self,
        mountPoint: bytes,
        parents: WorkingDirectoryParents,
        params: ResetParentCommitsParams,
    ) -> None:
        self.set_parents_calls.append(
            ResetParentsCommitsArgs(
                mount=mountPoint,
                parent1=parents.parent1,
                parent2=parents.parent2,
                hg_root_manifest=params.hgRootManifest,
                rootIdOptions=params.rootIdOptions,
            )
        )

    # TODO: this returns gobbledy gook at the moment.  improve to return a realistic value
    def getRegexCounters(self, regexValue: str) -> Dict[str, int]:
        result = {
            "prjfs.something": 1,
            "prjfs.somethingelse": 2,
            "prjfs.somethingelseelse": 3,
        }
        return result

    def debugInodeStatus(
        self,
        mountPoint: bytes,
        path: bytes,
        flags: int,
        sync: SyncBehavior,
    ) -> List[TreeInodeEntryDebugInfo]:
        return []

    def getSHA1(
        self, mountPoint: bytes, paths: List[bytes], sync: SyncBehavior
    ) -> List[SHA1Result]:
        return []

    def getStatInfo(self, params: GetStatInfoParams) -> InternalStats:
        mount_paths = [mount.mount_point for mount in self._mount_table.mounts]
        mount_point_info = {
            path: self._path_mount_inode_info[path] for path in mount_paths
        }
        return InternalStats(mountPointInfo=mount_point_info)

    def getCounter(self, key: str) -> int:
        return self._counter_values[key]

    def debugInvalidateNonMaterialized(
        self, params: DebugInvalidateRequest
    ) -> DebugInvalidateResponse:
        return DebugInvalidateResponse(numInvalidated=0)

    def getScmStatusV2(self, params: GetScmStatusParams) -> GetScmStatusResult:
        return GetScmStatusResult(status=ScmStatus(entries={}))

    def getCurrentSnapshotInfo(
        self, params: GetCurrentSnapshotInfoRequest
    ) -> GetCurrentSnapshotInfoResponse:
        return GetCurrentSnapshotInfoResponse(fid=None)

    def matchFilesystem(
        self, params: MatchFileSystemRequest
    ) -> MatchFileSystemResponse:
        return MatchFileSystemResponse(results=[])
