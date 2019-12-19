#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import subprocess
from pathlib import Path

from eden.integration.lib import hgrepo
from facebook.eden.ttypes import WorkingDirectoryParents

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
# pyre-fixme[13]: Attribute `backing_repo` is never initialized.
# pyre-fixme[13]: Attribute `backing_repo_name` is never initialized.
# pyre-fixme[13]: Attribute `config_variant_name` is never initialized.
# pyre-fixme[13]: Attribute `repo` is never initialized.
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

    def test_eden_doctor_fixes_valid_mismatched_parents(self) -> None:
        # this specifically tests when EdenFS and Mercurial are out of sync,
        # but and mercurial does know about EdenFS's WCP
        mount_path = Path(self.mount)

        # set eden to point at the first commit, while keeping mercurial at the
        # second commit
        parents = WorkingDirectoryParents(parent1=self.commit1.encode("utf-8"))
        with self.eden.get_thrift_client() as client:
            client.resetParentCommits(mountPoint=bytes(mount_path), parents=parents)

        with self.assertRaises(hgrepo.HgError) as status_context:
            self.repo.status()

        self.assertIn(
            b"requested parent commit is out-of-date", status_context.exception.stderr
        )

        # hg whereami reads eden's SNAPSHOT file
        eden_parent = self.hg("whereami").strip("\n")
        hg_parent = self.hg("log", "-r.", "-T{node}")

        # make sure that eden and mercurial are out of sync
        self.assertNotEqual(eden_parent, hg_parent)

        cmd_result = self.eden.run_unchecked("doctor", "-n", stdout=subprocess.PIPE)
        error_msg = (
            "mercurial's parent commit is %s, but Eden's internal parent commit is %s"
            % (self.commit2, self.commit1)
        )
        self.assertIn(error_msg.encode("utf-8"), cmd_result.stdout)

        # run eden doctor and make sure eden and mercurial are in sync again
        fixed_result = self.eden.run_unchecked("doctor", stdout=subprocess.PIPE)
        self.assertIn(b"Successfully fixed 1 problem", fixed_result.stdout)

        eden_parent_fixed = self.hg("whereami").strip("\n")
        hg_parent_fixed = self.hg("log", "-r.", "-T{node}")
        self.assertEqual(eden_parent_fixed, hg_parent_fixed)

        # Since Eden's snapshot file pointed to a known commit, it should pick
        # Eden's parent as the new parent
        self.assertEqual(eden_parent, hg_parent_fixed)

    def test_eden_doctor_fixes_invalid_mismatched_parents(self) -> None:
        # this specifically tests when EdenFS and Mercurial are out of sync,
        # but Mercurial does not know about Eden's WCP

        mount_path = Path(self.mount)

        corrupt_commit = b"9" * 40
        parents = WorkingDirectoryParents(parent1=corrupt_commit)

        # point eden to a random commit
        with self.eden.get_thrift_client() as client:
            client.resetParentCommits(mountPoint=bytes(mount_path), parents=parents)

        with self.assertRaises(hgrepo.HgError) as status_context:
            self.repo.status()

        self.assertIn(
            b"requested parent commit is out-of-date", status_context.exception.stderr
        )

        # hg whereami reads eden's SNAPSHOT file
        eden_parent = self.hg("whereami").strip("\n")
        hg_parent = self.hg("log", "-r.", "-T{node}")

        # make sure that eden and mercurial are out of sync
        self.assertNotEqual(eden_parent, hg_parent)

        cmd_result = self.eden.run_unchecked("doctor", "-n", stdout=subprocess.PIPE)
        self.assertIn(b"Eden's snapshot file points to a bad commit", cmd_result.stdout)

        # run eden doctor and make sure eden and mercurial are in sync again
        fixed_result = self.eden.run_unchecked("doctor", stdout=subprocess.PIPE)
        self.assertIn(b"Successfully fixed 1 problem", fixed_result.stdout)

        eden_parent_fixed = self.hg("whereami").strip("\n")
        hg_parent_fixed = self.hg("log", "-r.", "-T{node}")
        self.assertEqual(eden_parent_fixed, hg_parent_fixed)

        # Since Eden's snapshot file pointed to a bad commit, it should pick
        # mercurial's parent as the new parent
        self.assertEqual(hg_parent, hg_parent_fixed)
