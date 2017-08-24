#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from .lib.hg_extension_test_base import hg_test
from ..lib import hgrepo


@hg_test
class RollbackTest:
    def populate_backing_repo(self, repo):
        repo.write_file('first', '')
        self._commit1 = repo.commit('first commit')

    def test_amend_with_editor_failure_should_trigger_rollback(self):
        original_commits = self.repo.log()

        self.repo.write_file('first', 'THIS IS CHANGED')
        self.assert_status({'first': 'M'})

        with self.assertRaises(hgrepo.HgError) as context:
            self.hg('amend', '--edit', '--config', 'ui.editor=/bin/false')
        expected_msg = 'transaction abort!\n  rollback completed\n'
        self.assertIn(expected_msg, str(context.exception))

        self.assertEqual(
            original_commits,
            self.repo.log(),
            msg='Failed editor should abort the change and '
            'leave Hg in the original state.'
        )
