#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import abc
import binascii
import os
import subprocess
from pathlib import Path
from typing import Dict, List, Optional, Tuple, Type

import eden.dirstate
import facebook.eden.ttypes as eden_ttypes
from eden.cli import hg_util
from eden.cli.config import EdenCheckout
from eden.cli.doctor.problem import FixableProblem, ProblemTracker, UnexpectedCheckError
from thrift.Thrift import TApplicationException


class HgChecker:
    errors: List[str] = []

    def __init__(self, checkout: EdenCheckout) -> None:
        self.checkout = checkout

    def check(self) -> bool:
        self.errors = self.check_for_error()
        return not self.errors

    @abc.abstractmethod
    def check_for_error(self) -> List[str]:
        """Check for errors.

        Returns a list of errors, or an empty list if no problems were found.
        """
        raise NotImplementedError()

    @abc.abstractmethod
    def repair(self) -> None:
        raise NotImplementedError()


class HgFileChecker(HgChecker):
    def __init__(self, checkout: EdenCheckout, name: str) -> None:
        super().__init__(checkout)
        self.name = name
        self.problem: Optional[str] = None

    @property
    def path(self) -> Path:
        return self.checkout.path / ".hg" / self.name

    @property
    def short_path(self) -> str:
        return os.path.join(".hg", self.name)

    def check_for_error(self) -> List[str]:
        try:
            data = self.path.read_bytes()
        except IOError as ex:
            return [f"error reading {self.short_path}: {ex}"]

        return self.check_data(data)

    def check_data(self, data: bytes) -> List[str]:
        return []


class DirstateChecker(HgFileChecker):
    _null_commit_id = 20 * b"\x00"

    _old_snapshot: Optional[bytes] = None
    _old_dirstate_parents: Optional[Tuple[bytes, bytes]] = None
    _tuples_dict: Dict[bytes, Tuple[str, int, int]] = {}
    _copymap: Dict[bytes, bytes] = {}
    _new_parents: Optional[Tuple[bytes, bytes]] = None

    def __init__(self, checkout: EdenCheckout) -> None:
        super().__init__(checkout, "dirstate")

    def check_for_error(self) -> List[str]:
        errors: List[str] = []

        self._get_old_dirstate_info(errors)
        self._get_old_snapshot(errors)
        self._new_parents = self._select_new_parents(errors)

        # If we need to update state make sure we reported an error
        if (
            self._new_parents != self._old_dirstate_parents
            # pyre-fixme[16]: `Optional` has no attribute `__getitem__`.
            or self._new_parents[0] != self._old_snapshot
        ):
            assert errors

        return errors

    def _get_old_dirstate_info(self, errors: List[str]) -> None:
        # Read the data from the dirstate file
        try:
            with self.path.open("rb") as f:
                parents, tuples_dict, copymap = eden.dirstate.read(f, str(self.path))
            self._old_dirstate_parents = parents
            self._tuples_dict = {os.fsencode(k): v for k, v in tuples_dict.items()}
            self._copymap = {os.fsencode(k): os.fsencode(v) for k, v in copymap.items()}
        except IOError as ex:
            errors.append(f"error reading {self.short_path}: {ex}")
            return
        except eden.dirstate.DirstateParseException as ex:
            errors.append(f"error parsing {self.short_path}: {ex}")
            return

        # Make sure the commits are valid, and discard them otherwise
        old_p0 = self._check_commit(errors, parents[0], "mercurial's p0 commit")
        old_p1 = self._check_commit(errors, parents[1], "mercurial's p1 commit")
        if old_p0 is None:
            self._old_dirstate_parents = None
        else:
            if old_p1 is None:
                old_p1 = self._null_commit_id
            self._old_dirstate_parents = (old_p0, old_p1)

    def _get_old_snapshot(self, errors: List[str]) -> None:
        # Get the commit ID from the snapshot file
        try:
            snapshot_hex = self.checkout.get_snapshot()
            self._old_snapshot = binascii.unhexlify(snapshot_hex)
        except Exception as ex:
            errors.append(f"error parsing Eden snapshot ID: {ex}")
            return

        self._old_snapshot = self._check_commit(
            errors,
            # pyre-fixme[6]: Expected `bytes` for 2nd param but got `Optional[bytes]`.
            self._old_snapshot,
            "Eden's snapshot file",
        )

    def _check_commit(
        self, errors: List[str], commit: bytes, name: str
    ) -> Optional[bytes]:
        if self._is_commit_hash_valid(commit):
            return commit
        commit_hex = self._commit_hex(commit)
        errors.append(f"{name} points to a bad commit: {commit_hex}")
        return None

    def _select_new_parents(self, errors: List[str]) -> Tuple[bytes, bytes]:
        if self._old_snapshot is None and self._old_dirstate_parents is None:
            last_resort = self._get_last_resort_commit()
            return (last_resort, self._null_commit_id)
        elif self._old_dirstate_parents is None:
            assert self._old_snapshot is not None  # to make mypy happy
            return (self._old_snapshot, self._null_commit_id)
        else:
            if (
                self._old_snapshot is not None
                # pyre-fixme[16]: `Optional` has no attribute `__getitem__`.
                and self._old_snapshot != self._old_dirstate_parents[0]
            ):
                p0_hex = self._commit_hex(self._old_dirstate_parents[0])
                # pyre-fixme[6]: Expected `bytes` for 1st param but got
                #  `Optional[bytes]`.
                snapshot_hex = self._commit_hex(self._old_snapshot)
                errors.append(
                    f"mercurial's parent commit is {p0_hex}, but Eden's internal "
                    f"parent commit is {snapshot_hex}"
                )
            # pyre-fixme[7]: Expected `Tuple[bytes, bytes]` but got
            #  `Optional[Tuple[bytes, bytes]]`.
            return self._old_dirstate_parents

    def repair(self) -> None:
        # If the .hg directory was missing entirely check_for_error() won't have been
        # called yet.  Call it now to compute self._new_parents
        if self._new_parents is None:
            self.check_for_error()
        assert self._new_parents is not None

        if self._new_parents != self._old_dirstate_parents:
            with self.path.open("wb") as f:
                eden.dirstate.write(
                    f,
                    # pyre-fixme[6]: Expected `Tuple[bytes, bytes]` for 2nd param
                    #  but got `Optional[Tuple[bytes, bytes]]`.
                    self._new_parents,
                    self._tuples_dict,
                    self._copymap,
                )

        # pyre-fixme[16]: `Optional` has no attribute `__getitem__`.
        if self._new_parents[0] != self._old_snapshot:
            parents = eden_ttypes.WorkingDirectoryParents(parent1=self._new_parents[0])
            if self._new_parents[1] != self._null_commit_id:
                parents.parent2 = self._new_parents[1]
            with self.checkout.instance.get_thrift_client() as client:
                client.resetParentCommits(bytes(self.checkout.path), parents)

    def _commit_hex(self, commit: bytes) -> str:
        return binascii.hexlify(commit).decode("utf-8")

    def _is_commit_hash_valid(self, commit_hash: bytes) -> bool:
        # The null commit ID is always valid
        if commit_hash == self._null_commit_id:
            return True

        try:
            with self.checkout.instance.get_thrift_client() as client:
                client.getScmStatusBetweenRevisions(
                    bytes(self.checkout.path), commit_hash, commit_hash
                )
            return True
        except (TApplicationException, eden_ttypes.EdenError) as ex:
            if "RepoLookupError: unknown revision" in str(ex):
                return False
            raise

    def _get_last_resort_commit(self) -> bytes:
        try:
            return get_tip_commit_hash(self.checkout.path)
        except Exception:
            return self._null_commit_id


class HgrcChecker(HgFileChecker):
    def __init__(self, checkout: EdenCheckout) -> None:
        super().__init__(checkout, "hgrc")

    def repair(self) -> None:
        hgrc_data = hg_util.get_hgrc_data(self.checkout)
        self.path.write_text(hgrc_data)


class RequiresChecker(HgFileChecker):
    def __init__(self, checkout: EdenCheckout) -> None:
        super().__init__(checkout, "requires")

    def check_data(self, data: bytes) -> List[str]:
        requirements = data.splitlines()
        if b"eden" not in requirements:
            return [".hg/requires file does not include Eden as a requirement"]
        return []

    def repair(self) -> None:
        hgrc_data = hg_util.get_requires_data(self.checkout)
        self.path.write_text(hgrc_data)


class SharedPathChecker(HgFileChecker):
    def __init__(self, checkout: EdenCheckout) -> None:
        super().__init__(checkout, "sharedpath")

    def check_data(self, data: bytes) -> List[str]:
        # TODO: make sure the sharedpath file points to a valid .hg directory that
        # does not use Eden itself.  However, we can't fix errors about the sharedpath
        # file pointing to a bad repo, so those should probably be reported as
        # completely separate problems to the ProblemTracker.
        #
        # backing_repo = Path(os.fsdecode(data))
        return []

    def repair(self) -> None:
        backing_hg_dir = hg_util.get_backing_hg_dir(self.checkout)
        self.path.write_bytes(bytes(backing_hg_dir))


class SharedChecker(HgFileChecker):
    def __init__(self, checkout: EdenCheckout) -> None:
        super().__init__(checkout, "shared")

    def check_data(self, data: bytes) -> List[str]:
        # This file normally contains "bookmarks" for most users, but its fine
        # if users don't have anything here if they don't want to share bookmarks.
        # Therefore we don't do any other validation of the contents of this file.
        return []

    def repair(self) -> None:
        self.path.write_text("bookmarks\n")


class BookmarksChecker(HgFileChecker):
    def __init__(self, checkout: EdenCheckout) -> None:
        super().__init__(checkout, "bookmarks")

    def repair(self) -> None:
        self.path.touch()


class BranchChecker(HgFileChecker):
    def __init__(self, checkout: EdenCheckout) -> None:
        super().__init__(checkout, "branch")

    def repair(self) -> None:
        self.path.write_text("default\n")


def get_tip_commit_hash(repo: Path) -> bytes:
    # Try to get the tip commit ID.  If that fails, use the null commit ID.
    args = ["hg", "log", "-T", "{node}", "-r", "tip"]
    env = dict(os.environ, HGPLAIN="1")
    result = subprocess.run(
        args,
        env=env,
        cwd=str(repo),
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    return binascii.unhexlify(result.stdout.strip())


def check_hg(tracker: ProblemTracker, checkout: EdenCheckout) -> None:
    checker_classes: List[Type[HgChecker]] = [
        DirstateChecker,
        HgrcChecker,
        RequiresChecker,
        SharedPathChecker,
        SharedChecker,
        BookmarksChecker,
        BranchChecker,
    ]
    checkers = [checker_class(checkout) for checker_class in checker_classes]

    hg_path = checkout.path / ".hg"
    if not os.path.exists(hg_path):
        description = f"Missing hg directory: {checkout.path}/.hg"
        tracker.add_problem(HgDirectoryError(checkout, checkers, description))
        return

    bad_checkers: List[HgChecker] = []
    for checker in checkers:
        try:
            if checker.check():
                continue
            bad_checkers.append(checker)
        except Exception:
            tracker.add_problem(UnexpectedCheckError())

    if bad_checkers:
        msg = (
            f"No contents present in hg directory: {checkout.path}/.hg"
            if len(bad_checkers) == len(checkers)
            else None
        )
        tracker.add_problem(HgDirectoryError(checkout, bad_checkers, msg))


class HgDirectoryError(FixableProblem):
    def __init__(
        self,
        checkout: EdenCheckout,
        checkers: List[HgChecker],
        description: Optional[str] = None,
    ) -> None:
        self._checkout = checkout
        self._checkers = checkers
        self._description = description

    def description(self) -> str:
        if self._description is not None:
            # pyre-fixme[7]: Expected `str` but got `Optional[str]`.
            return self._description
        all_errors = []
        for checker in self._checkers:
            all_errors.extend(checker.errors)
        problems = "\n  ".join(all_errors)
        return (
            f"Found inconsistent/missing data in {self._checkout.path}/.hg:\n  "
            + problems
        )

    def dry_run_msg(self) -> str:
        return f"Would repair hg directory contents for {self._checkout.path}"

    def start_msg(self) -> str:
        return f"Repairing hg directory contents for {self._checkout.path}"

    def perform_fix(self) -> None:
        hg_path = self._checkout.path / ".hg"

        # Make sure the hg directory exists
        hg_path.mkdir(exist_ok=True)

        for checker in self._checkers:
            checker.repair()
