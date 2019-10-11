#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import logging
import os
from typing import Dict, Optional

from eden.integration.lib import hgrepo

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
# pyre-fixme[38]: `UndoTest` does not implement all inherited abstract methods.
# pyre-fixme[13]: Attribute `backing_repo` is never initialized.
# pyre-fixme[13]: Attribute `backing_repo_name` is never initialized.
# pyre-fixme[13]: Attribute `config_variant_name` is never initialized.
# pyre-fixme[13]: Attribute `repo` is never initialized.
class UndoTest(EdenHgTestCase):
    # pyre-fixme[13]: Attribute `commit1` is never initialized.
    commit1: str

    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        repo.write_file("src/common/foo/test.txt", "testing\n")
        self.commit1 = repo.commit("Initial commit.")

    def edenfs_logging_settings(self) -> Optional[Dict[str, str]]:
        edenfs_log_levels = {}

        log = logging.getLogger("eden.test.undo")
        if log.getEffectiveLevel() >= logging.DEBUG:
            edenfs_log_levels["eden.fs.inodes.TreeInode"] = "DBG5"

        return edenfs_log_levels

    def test_undo_commit_with_new_dir(self) -> None:
        log = logging.getLogger("eden.test.undo")

        # Add a new file in a new directory
        log.debug("=== commit 1: %s", self.commit1)
        base_dir = "src/common/foo"
        new_dir = "src/common/foo/newdir"
        new_file = "src/common/foo/newdir/code.c"
        self.mkdir(new_dir)
        self.write_file(new_file, "echo hello world\n")
        # Add the file and create a new commit
        log.debug("=== hg add")
        self.hg("add", new_file)
        log.debug("=== hg commit")
        commit2 = self.repo.commit("Added newdir\n")
        log.debug("=== commit 2: %s", commit2)
        self.assert_status_empty()
        self.assertNotEqual(self.repo.get_head_hash(), self.commit1)

        # Use 'hg undo' to revert the commit
        log.debug("=== hg undo")
        self.hg("undo")
        log.debug("=== undo done")
        self.assert_status_empty()
        log.debug("=== new head: %s", self.repo.get_head_hash())
        self.assertEqual(self.repo.get_head_hash(), self.commit1)

        # listdir() should only return test.txt now, and not newdir
        dir_entries = os.listdir(self.get_path(base_dir))
        self.assertEqual(dir_entries, ["test.txt"])

        # stat() calls should fail with ENOENT for newdir
        # This exercises a regression we have had in the past where we did not
        # flush the kernel inode cache entries properly, causing listdir()
        # to report the contents correctly but stat() to report that the
        # directory still existed.
        self.assertFalse(os.path.exists(self.get_path(new_dir)))
