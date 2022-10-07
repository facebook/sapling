#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import subprocess
from pathlib import Path

from eden.integration.lib import hgrepo
from facebook.eden.ttypes import ResetParentCommitsParams, WorkingDirectoryParents

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
# pyre-ignore[13]: T62487924
class DoctorTest(EdenHgTestCase):
    commit1: str
    commit2: str

    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        repo.write_file("letters", "a\nb\nc\n")
        repo.write_file("numbers", "1\n2\n3\n")
        self.commit1 = repo.commit("Initial commit.")

        repo.write_file("letters", "a\n")
        repo.write_file("numbers", "1\n")
        self.commit2 = repo.commit("New commit.")

    def run_doctor(self, dry_run: bool) -> subprocess.CompletedProcess:
        args = ["doctor", "--current-edenfs-only"]
        if dry_run:
            args.append("-n")
        return self.eden.run_unchecked(*args, stdout=subprocess.PIPE)

    def test_eden_doctor_doesnt_crash_on_merge_commit(self) -> None:
        self.repo.write_file("hello.txt", "one")
        commit3 = self.repo.commit("Commit three")

        self.repo.update(self.commit2)
        self.repo.write_file("hello.txt", "two")
        commit4 = self.repo.commit("Commit four")

        with self.assertRaises(hgrepo.HgError):
            self.hg("rebase", "-r", commit4, "-d", commit3)
        self.assert_unresolved(unresolved=["hello.txt"])

        cmd_result = self.run_doctor(dry_run=False)
        self.assertNotIn(b"AssertionError", cmd_result.stdout)

    def get_eden_parent(self) -> str:
        data = self.eden.run_cmd("debug", "parents", cwd=self.mount)
        return data.rstrip()

    def test_eden_doctor_fixes_valid_mismatched_parents(self) -> None:
        # this specifically tests when EdenFS and Mercurial are out of sync,
        # but and mercurial does know about EdenFS's WCP
        mount_path = Path(self.mount)

        # set eden to point at the first commit, while keeping mercurial at the
        # second commit
        parents = WorkingDirectoryParents(parent1=self.commit1.encode("utf-8"))
        params = ResetParentCommitsParams()
        with self.eden.get_thrift_client_legacy() as client:
            client.resetParentCommits(
                mountPoint=bytes(mount_path), parents=parents, params=params
            )

        with self.assertRaises(hgrepo.HgError) as status_context:
            self.repo.status()

        self.assertIn(
            b"requested parent commit is out-of-date", status_context.exception.stderr
        )

        eden_parent = self.get_eden_parent()
        hg_parent = self.hg("log", "-r.", "-T{node}")

        # make sure that eden and mercurial are out of sync
        self.assertNotEqual(eden_parent, hg_parent)

        cmd_result = self.run_doctor(dry_run=True)
        error_msg = (
            "mercurial's parent commit is %s, but Eden's internal parent commit is %s"
            % (self.commit2, self.commit1)
        )
        self.assertIn(error_msg.encode("utf-8"), cmd_result.stdout)

        # run eden doctor and make sure eden and mercurial are in sync again
        fixed_result = self.run_doctor(dry_run=False)
        self.assertIn(b"Successfully fixed 1 problem", fixed_result.stdout)

        eden_parent_fixed = self.get_eden_parent()
        hg_parent_fixed = self.hg("log", "-r.", "-T{node}")
        self.assertEqual(eden_parent_fixed, hg_parent_fixed)

        # Since Eden's snapshot file pointed to a known commit, it should pick
        # Eden's parent as the new parent
        self.assertEqual(eden_parent, hg_parent_fixed)

    def test_eden_doctor_fixes_bad_dirstate_file(self) -> None:
        repo_path = Path(self.repo.path)
        # replace .hg/dirstate with an empty file
        (repo_path / ".hg" / "dirstate").write_bytes(b"")

        cmd_result = self.run_doctor(dry_run=True)
        doctor_out = cmd_result.stdout.decode("utf-8")
        self.assertIn(
            f"Found inconsistent/missing data in {repo_path / '.hg'}", doctor_out
        )
        self.assertIn(f"error parsing .hg{os.sep}dirstate", doctor_out)

        fixed_result = self.run_doctor(dry_run=False)
        self.assertIn(b"Successfully fixed 1 problem", fixed_result.stdout)

        fixed_parent = self.hg("log", "-r.", "-T{node}")
        self.assertEqual(fixed_parent, self.commit2)
