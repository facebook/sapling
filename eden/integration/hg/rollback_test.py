#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from eden.integration.lib import hgrepo

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
class RollbackTest(EdenHgTestCase):
    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        repo.write_file("first", "")
        self._commit1 = repo.commit("first commit")

    def test_commit_with_precommit_failure_should_trigger_rollback(self) -> None:
        original_commits = self.repo.log()

        self.repo.write_file("first", "THIS IS CHANGED")
        self.assert_status({"first": "M"})

        with self.assertRaises(hgrepo.HgError) as context:
            self.hg(
                "commit",
                "-m",
                "Precommit hook should fail, causing rollback.",
                "--config",
                "hooks.pretxncommit=false",
            )
        expected_msg = (
            b"transaction abort!\nrollback completed\n"
            b"abort: pretxncommit hook exited with status 1\n"
        )
        self.assertIn(expected_msg, context.exception.stderr)

        self.assertEqual(
            original_commits,
            self.repo.log(),
            msg="Failed precommit hook should abort the change and "
            "leave Hg in the original state.",
        )
        self.assert_status({"first": "M"})
