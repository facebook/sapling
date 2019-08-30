#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import logging
import os
from typing import Dict, List

from eden.integration.lib.hgrepo import HgRepository
from eden.integration.lib.util import gen_tree

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
# pyre-fixme[38]: `StatusDeadlockTest` does not implement all inherited abstract
#  methods.
# pyre-fixme[13]: Attribute `backing_repo` is never initialized.
# pyre-fixme[13]: Attribute `backing_repo_name` is never initialized.
# pyre-fixme[13]: Attribute `config_variant_name` is never initialized.
# pyre-fixme[13]: Attribute `repo` is never initialized.
class StatusDeadlockTest(EdenHgTestCase):
    """
    Test running an "hg status" command that needs to import many directories
    and .gitignore files.

    This attempts to exercise a deadlock issue we had in the past where all of
    the EdenServer thread pool threads would be blocked waiting on operations
    that needed a thread from this pool to complete.  Eden wouldn't be able to
    make forward progress from this state.
    """

    # pyre-fixme[13]: Attribute `commit1` is never initialized.
    commit1: str
    expected_status: Dict[str, str] = {}

    def edenfs_logging_settings(self) -> Dict[str, str]:
        levels = {"eden": "DBG2"}
        if logging.getLogger().getEffectiveLevel() <= logging.DEBUG:
            levels["eden.fs.store.hg"] = "DBG9"
        return levels

    def populate_backing_repo(self, repo: HgRepository) -> None:
        logging.debug("== populate_backing_repo")

        # By default repo.write_file() also calls "hg add" on the path.
        # Unfortunately "hg add" is really slow.  Disable calling it one at a
        # time on these files as we write them.  We'll make a single "hg add"
        # call at the end with all paths in a single command.
        new_files = []

        fanouts = [4, 4, 4, 4]

        def populate_dir(path: str) -> None:
            logging.debug("populate %s", path)
            test_path = os.path.join(path, "test.txt")
            repo.write_file(test_path, f"test\n{path}\n", add=False)
            new_files.append(test_path)

            gitignore_path = os.path.join(path, ".gitignore")
            gitignore_contents = f"*.log\n/{path}/foo.txt\n"
            repo.write_file(gitignore_path, gitignore_contents, add=False)
            new_files.append(gitignore_path)

        gen_tree("src", fanouts, populate_dir, populate_dir)

        self._hg_add_many(repo, new_files)

        self.commit1 = repo.commit("Initial commit.")
        logging.debug("== created initial commit")

        new_files = []

        def create_new_file(path: str) -> None:
            logging.debug("add new file in %s", path)
            new_path = os.path.join(path, "new.txt")
            repo.write_file(new_path, "new\n", add=False)
            new_files.append(new_path)
            self.expected_status[new_path] = "?"

        gen_tree("src", fanouts, create_new_file)
        self._hg_add_many(repo, new_files)

        # pyre-fixme[16]: `StatusDeadlockTest` has no attribute `commit2`.
        self.commit2 = repo.commit("Initial commit.")
        logging.debug("== created second commit")

    def _hg_add_many(self, repo: HgRepository, paths: List[str]) -> None:
        # Call "hg add" with at most chunk_size files at a time
        chunk_size = 250
        for n in range(0, len(paths), chunk_size):
            logging.debug("= add %d/%d", n, len(paths))
            repo.add_files(paths[n : n + chunk_size])

    def test(self) -> None:
        # Reset our working directory parent from commit2 to commit1.
        # This forces us to have an unclean status, but eden won't need
        # to load the source control state yet.  It won't load the affected
        # trees until we run "hg status" below.
        self.hg("reset", "--keep", self.commit1)

        # Now run "hg status".
        # This will cause eden to import all of the trees and .gitignore
        # files as it performs the status operation.
        logging.debug("== checking status")
        self.assert_status(self.expected_status)
