#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import configparser
import os
import sys

from eden.integration.lib import hgrepo

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
# pyre-ignore[13]: T62487924
class SymlinkTest(EdenHgTestCase):
    def apply_hg_config_variant(self, hgrc: configparser.ConfigParser) -> None:
        super().apply_hg_config_variant(hgrc)
        hgrc["experimental"]["run-python-hooks-via-pyhook"] = "true"
        hgrc["hooks"] = {
            "update.post-clone-test": "echo ran > update-hook-ran",
            "update.python-post-clone-test": (
                "python:update_hook.py:write_current_commit"
            ),
        }

    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        repo.write_file("contents1", "c1\n")
        repo.write_file("contents2", "c2\n")
        repo.write_file(
            "update_hook.py",
            """\
from pathlib import Path


def write_current_commit(io, repo, **kwargs):
    commit = repo.working_parent_nodes()[0].hex()
    Path(repo.path, "python-update-hook-ran").write_text(commit)
""",
        )
        repo.symlink("symlink", "contents1")
        repo.commit("Initial commit.")

    def test_update_hook(self) -> None:
        with open(os.path.join(self.mount, "update-hook-ran")) as f:
            self.assertEqual("ran", f.read().strip())

    def test_python_update_hook(self) -> None:
        with open(os.path.join(self.mount, "python-update-hook-ran")) as f:
            self.assertEqual(self.backing_repo.get_head_hash(), f.read())

    def test_post_clone_permissions(self) -> None:
        st = os.stat(os.path.join(self.mount, ".hg"))
        expected_mode = 0o777 if sys.platform == "win32" else 0o755
        self.assertEqual(st.st_mode & 0o777, expected_mode)
