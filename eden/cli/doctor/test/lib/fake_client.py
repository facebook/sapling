#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import binascii
from pathlib import Path
from typing import Callable, List, NamedTuple, Optional, Union

import facebook.eden.ttypes as eden_ttypes


class ResetParentsCommitsArgs(NamedTuple):
    mount: bytes
    parent1: bytes
    parent2: Optional[bytes]


class FakeClient:
    commit_checker: Optional[Callable[[bytes, str], bool]] = None

    def __init__(self):
        self._mounts = []
        self.set_parents_calls: List[ResetParentsCommitsArgs] = []

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_value, exc_traceback):
        pass

    def change_mount_state(self, path: Path, state: Optional[eden_ttypes.MountState]):
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
        self, mountPoint: bytes, parents: eden_ttypes.WorkingDirectoryParents
    ):
        self.set_parents_calls.append(
            ResetParentsCommitsArgs(
                mount=mountPoint, parent1=parents.parent1, parent2=parents.parent2
            )
        )

    def getScmStatus(
        self,
        mountPoint: Optional[bytes] = None,
        listIgnored: Optional[bool] = None,
        commit: Optional[bytes] = None,
    ) -> Optional[eden_ttypes.ScmStatus]:
        assert mountPoint is not None
        self._check_commit_valid(mountPoint, commit)
        return None

    def getScmStatusBetweenRevisions(
        self,
        mountPoint: Optional[bytes] = None,
        oldHash: Optional[bytes] = None,
        newHash: Optional[bytes] = None,
    ) -> Optional[eden_ttypes.ScmStatus]:
        assert mountPoint is not None
        self._check_commit_valid(mountPoint, oldHash)
        self._check_commit_valid(mountPoint, newHash)
        return None

    def _check_commit_valid(self, path: bytes, commit: Union[None, bytes, str]):
        if self.commit_checker is None:
            return

        if commit is None:
            return
        if isinstance(commit, str):
            commit_hex = commit
        else:
            commit_hex = binascii.hexlify(commit).decode("utf-8")

        if not self.commit_checker(path, commit_hex):
            raise eden_ttypes.EdenError(
                message=f"RepoLookupError: unknown revision {commit_hex}"
            )
