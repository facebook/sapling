#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from eden.integration.lib import hgrepo

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
class SparseTest(EdenHgTestCase):
    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        repo.write_file("a_file.txt", "")
        repo.commit("first commit")

    def test_sparse(self) -> None:
        """Verify that we show a reasonable error if someone has managed
        to load the sparse extension, rather than an ugly stack trace"""

        for sub in [
            "clear",
            "cwd",
            "delete",
            "disable",
            "enable",
            "exclude",
            "explain",
            "files someprofile",
            "importrules",
            "include",
            "show",
            "list",
            "refresh",
            "reset",
        ]:
            with self.assertRaises(hgrepo.HgError) as context:
                self.hg("--config", "extensions.sparse=", "sparse", *sub.split())
            self.assertIn(
                "don't need sparse profiles",
                # pyre-fixme[16]: `_E` has no attribute `stderr`.
                context.exception.stderr.decode("utf-8", errors="replace"),
            )
