#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from textwrap import dedent

from eden.integration.lib import hgrepo

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
# pyre-fixme[13]: Attribute `backing_repo` is never initialized.
# pyre-fixme[13]: Attribute `backing_repo_name` is never initialized.
# pyre-fixme[13]: Attribute `config_variant_name` is never initialized.
# pyre-fixme[13]: Attribute `repo` is never initialized.
class DebugHgGetDirstateTupleTest(EdenHgTestCase):
    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        repo.write_file("hello", "hola\n")
        repo.write_file("dir/file", "blah\n")
        repo.commit("Initial commit.")

    def test_get_dirstate_tuple_normal_file(self) -> None:
        output = self.eden.run_cmd(
            "debug", "hg_get_dirstate_tuple", self.get_path("hello")
        )
        expected = dedent(
            """\
        hello
            status = Normal
            mode = 0o100644
            mergeState = NotApplicable
        """
        )
        self.assertEqual(expected, output)
