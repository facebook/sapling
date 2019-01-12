#!/usr/bin/env python3
#
# Copyright (c) 2018-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import binascii
import errno
import os
import subprocess
from typing import Dict, List, Tuple

import eden.dirstate
import facebook.eden.ttypes as eden_ttypes
from eden.cli.config import EdenInstance
from eden.cli.doctor.problem import FixableProblem, Problem, ProblemTracker
from thrift.Thrift import TApplicationException


def check_snapshot_dirstate_consistency(
    tracker: ProblemTracker, instance: EdenInstance, path: str, snapshot_hex: str
) -> None:
    dirstate = os.path.join(path, ".hg", "dirstate")
    try:
        with open(dirstate, "rb") as f:
            parents, tuples_dict, copymap = eden.dirstate.read(f, dirstate)
    except OSError as ex:
        if ex.errno == errno.ENOENT:
            tracker.add_problem(MissingHgDirectory(path))
        else:
            tracker.add_problem(Problem(f"Unable to access {path}/.hg/dirstate: {ex}"))
        return
    except eden.dirstate.DirstateParseException as ex:
        tracker.add_problem(Problem(f"Unable to read {path}/.hg/dirstate: {ex}"))
        return

    p1_hex = binascii.hexlify(parents[0]).decode("utf-8")
    p2_hex = binascii.hexlify(parents[1]).decode("utf-8")
    null_hash_hex = 40 * "0"
    is_p2_hex_valid = True
    current_hex = snapshot_hex
    try:
        is_snapshot_hex_valid = is_commit_hash_valid(instance, path, snapshot_hex)
        current_hex = p1_hex
        is_p1_hex_valid = is_commit_hash_valid(instance, path, p1_hex)
        if p2_hex != null_hash_hex:
            current_hex = p2_hex
            is_p2_hex_valid = is_commit_hash_valid(instance, path, p2_hex)
    except Exception as ex:
        tracker.add_problem(
            Problem(
                f"Failed to get scm status for mount {path} "
                f"at revision {current_hex}:\n {ex}"
            )
        )
        return

    if is_p2_hex_valid is not True:
        p2_hex = null_hash_hex

    if snapshot_hex != p1_hex:
        if is_p1_hex_valid:
            new_parents = (binascii.unhexlify(p1_hex), binascii.unhexlify(p2_hex))
            tracker.add_problem(
                SnapshotMismatchError(instance, path, snapshot_hex, parents)
            )
        elif is_snapshot_hex_valid:
            new_parents = (binascii.unhexlify(snapshot_hex), binascii.unhexlify(p2_hex))
            tracker.add_problem(
                DirStateInvalidError(  # type: ignore
                    instance, path, p1_hex, new_parents, tuples_dict, copymap
                )
            )

    if (not is_snapshot_hex_valid) and (not is_p1_hex_valid):
        last_valid_commit_hash = get_tip_commit_hash()
        new_parents = (
            binascii.unhexlify(last_valid_commit_hash),
            binascii.unhexlify(p2_hex),
        )
        tracker.add_problem(
            DirStateInvalidError(  # type: ignore
                instance, path, p1_hex, new_parents, tuples_dict, copymap
            )
        )


class DirStateInvalidError(FixableProblem):
    def __init__(
        self,
        instance: EdenInstance,
        mount_path: str,
        invalid_commit_hash: str,
        hg_parents: Tuple[bytes, bytes],
        tuples_dict: Dict[bytes, Tuple[str, int, int]],
        copymap: Dict[bytes, bytes],
    ) -> None:
        self._instance = instance
        self._mount_path = mount_path
        self._invalid_commit_hash = invalid_commit_hash
        self._hg_parents = hg_parents
        self._tuples_dict = tuples_dict
        self._copymap = copymap

    def dirstate(self) -> str:
        return os.path.join(self._mount_path, ".hg", "dirstate")

    def p1_hex(self) -> str:
        return binascii.hexlify(self._hg_parents[0]).decode("utf-8")

    def description(self) -> str:
        return (
            f"mercurial's parent commit {self._invalid_commit_hash}"
            f" in {self.dirstate()} is invalid\n"
        )

    def dry_run_msg(self) -> str:
        return f"Would fix Eden to point to parent commit {self.p1_hex()}"

    def start_msg(self) -> str:
        return f"Fixing Eden to point to parent commit {self.p1_hex()}"

    def perform_fix(self) -> None:
        with open(self.dirstate(), "wb") as f:
            eden.dirstate.write(f, self._hg_parents, self._tuples_dict, self._copymap)

        parents = eden_ttypes.WorkingDirectoryParents(parent1=self._hg_parents[0])
        if self._hg_parents[1] != (20 * b"\0"):
            parents.parent2 = self._hg_parents[1]

        with self._instance.get_thrift_client() as client:
            client.resetParentCommits(self._mount_path.encode("utf-8"), parents)


def get_tip_commit_hash() -> str:
    args = ["hg", "log", "-T", "{node}\n", "-r", "tip"]
    env = dict(os.environ, HGPLAIN="1")
    stdout = subprocess.check_output(args, universal_newlines=True, env=env)
    lines: List[str] = list(filter(None, stdout.split("\n")))
    return lines[-1]


def is_commit_hash_valid(
    instance: EdenInstance, mount_path: str, commit_hash: str
) -> bool:
    try:
        with instance.get_thrift_client() as client:
            client.getScmStatus(
                os.fsencode(mount_path), False, commit_hash.encode("utf-8")
            )
            return True
    except TApplicationException as ex:
        if "RepoLookupError: unknown revision" in str(ex):
            return False
        raise


class SnapshotMismatchError(FixableProblem):
    def __init__(
        self,
        instance: EdenInstance,
        path: str,
        snapshot_hex: str,
        hg_parents: Tuple[bytes, bytes],
    ) -> None:
        self._instance = instance
        self._path = path
        self._snapshot_hex = snapshot_hex
        self._hg_parents = hg_parents

    def p1_hex(self) -> str:
        return binascii.hexlify(self._hg_parents[0]).decode("utf-8")

    def description(self) -> str:
        return (
            f"mercurial's parent commit for {self._path} is {self.p1_hex()},\n"
            f"but Eden's internal hash in its SNAPSHOT file is {self._snapshot_hex}.\n"
        )

    def dry_run_msg(self) -> str:
        return f"Would fix Eden to point to parent commit {self.p1_hex()}"

    def start_msg(self) -> str:
        return f"Fixing Eden to point to parent commit {self.p1_hex()}"

    def perform_fix(self) -> None:
        parents = eden_ttypes.WorkingDirectoryParents(parent1=self._hg_parents[0])
        if self._hg_parents[1] != (20 * b"\0"):
            parents.parent2 = self._hg_parents[1]

        with self._instance.get_thrift_client() as client:
            client.resetParentCommits(self._path.encode("utf-8"), parents)


class MissingHgDirectory(Problem):
    def __init__(self, path: str) -> None:
        remediation = f"""\
The most common cause of this is if you previously tried to manually remove this eden
mount with "rm -rf".  You should instead remove it using "eden rm {path}",
and can re-clone the checkout afterwards if desired."""
        super().__init__(f"{path}/.hg/dirstate is missing", remediation)
        self._path = path
