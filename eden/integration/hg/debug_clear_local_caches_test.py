#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os

from eden.integration.lib import hgrepo

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
# pyre-fixme[13]: Attribute `backing_repo` is never initialized.
# pyre-fixme[13]: Attribute `backing_repo_name` is never initialized.
# pyre-fixme[13]: Attribute `config_variant_name` is never initialized.
# pyre-fixme[13]: Attribute `repo` is never initialized.
class DebugClearLocalCachesTest(EdenHgTestCase):
    commit1: str
    commit2: str

    # These tests restart Eden and expect data to have persisted.
    def select_storage_engine(self) -> str:
        return "sqlite"

    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        repo.write_file("hello", "hola\n")
        self.commit1 = repo.commit("Initial commit.")

        repo.write_file("hello", "goodbye\n")
        repo.write_file("subdir/file2", "another file\n")
        self.commit2 = repo.commit("Commit 2")

    def test_update_still_works_after_clearing_caches(self) -> None:
        self.repo.update(self.commit2)
        self.eden.run_cmd("debug", "clear_local_caches")
        self.eden.restart()
        self.repo.update(self.commit1)
        self.repo.update(self.commit2)

    def read_all(self, *components):
        with open(os.path.join(self.mount, *components)) as f:
            return f.read()

    def test_contents_are_the_same_after_clearing_caches(self) -> None:
        self.repo.update(self.commit1)
        c1_hello = self.read_all("hello")

        self.repo.update(self.commit2)
        c2_hello = self.read_all("hello")
        c2_file2 = self.read_all("subdir", "file2")

        self.eden.run_cmd("debug", "clear_local_caches")
        self.eden.restart()

        self.repo.update(self.commit1)
        self.assertEqual(c1_hello, self.read_all("hello"))

        self.repo.update(self.commit2)
        self.assertEqual(c2_hello, self.read_all("hello"))
        self.assertEqual(c2_file2, self.read_all("subdir", "file2"))

    def test_contents_are_the_same_if_handle_is_held_open(self) -> None:
        # This test will fail if clear_local_caches deletes proxy hashes.
        # Graceful restarts effectively unload all inodes requiring them to be
        # reloaded, but the blob hashes are still known because the file is
        # still open.
        self.repo.update(self.commit2)
        with open(os.path.join(self.mount, "hello")) as c2_hello_file, open(
            os.path.join(self.mount, "subdir", "file2")
        ) as c2_file2_file:

            self.eden.run_cmd("debug", "clear_local_caches")
            self.eden.graceful_restart()
            self.eden.run_cmd("debug", "flush_cache", "hello", cwd=self.mount)
            self.eden.run_cmd(
                "debug", "flush_cache", os.path.join("subdir", "file2"), cwd=self.mount
            )

            self.assertEqual("goodbye\n", c2_hello_file.read())
            self.assertEqual("another file\n", c2_file2_file.read())
