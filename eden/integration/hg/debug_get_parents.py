#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from pathlib import Path

from eden.integration.lib import hgrepo
from facebook.eden.ttypes import WorkingDirectoryParents, ResetParentCommitsParams

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
# pyre-ignore[13]: T62487924
class DebugGetParentsTest(EdenHgTestCase):
    commit1: str
    commit2: str

    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        repo.write_file("letters", "a\nb\nc\n")
        repo.write_file("numbers", "1\n2\n3\n")
        self.commit1 = repo.commit("Initial commit.")

        repo.write_file("letters", "a\n")
        repo.write_file("numbers", "1\n")
        self.commit2 = repo.commit("New commit.")

    def test_same_parents(self) -> None:
        output = self.eden.run_cmd("debug", "parents", cwd=self.mount).strip("\n")
        self.assertEqual(output, self.commit2)

        output_hg = self.eden.run_cmd("debug", "parents", "--hg", cwd=self.mount)
        expected = "Mercurial p0: %s\nEdenFS snapshot: %s\n" % (
            self.commit2,
            self.commit2,
        )
        self.assertEqual(output_hg, expected)

    def test_different_parents(self) -> None:
        mount_path = Path(self.mount)

        # set eden to point at the first commit, while keeping mercurial at the
        # second commit
        parents = WorkingDirectoryParents(parent1=self.commit1.encode("utf-8"))
        params = ResetParentCommitsParams()
        with self.eden.get_thrift_client_legacy() as client:
            client.resetParentCommits(
                mountPoint=bytes(mount_path), parents=parents, params=params
            )

        output = self.eden.run_cmd("debug", "parents", cwd=self.mount).strip("\n")
        self.assertEqual(output, self.commit1)

        output_hg = self.eden.run_cmd("debug", "parents", "--hg", cwd=self.mount)
        expected = "Mercurial p0: %s\nEdenFS snapshot: %s\n" % (
            self.commit2,
            self.commit1,
        )
        self.assertEqual(output_hg, expected)
