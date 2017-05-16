#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import os

from .lib.hg_extension_test_base import HgExtensionTestBase
from ..lib import hgrepo


class HisteditTest(HgExtensionTestBase):
    def populate_backing_repo(self, repo):
        repo.write_file('first', '')
        self._commit1 = repo.commit('first commit')

        repo.write_file('second', '')
        self._commit2 = repo.commit('second commit')

        repo.write_file('third', '')
        self._commit3 = repo.commit('third commit')

    def test_stop_at_earlier_commit_in_the_stack_without_reordering(self):
        commits = self.repo.log()
        self.assertEqual(commits, [self._commit3, self._commit2, self._commit1])

        # histedit, stopping in the middle of the stack.
        commands_file = os.path.join(self.tmp_dir, 'histedit_commands.txt')
        with open(commands_file, 'w') as f:
            f.write('pick %s\n' % self._commit1)
            f.write('stop %s\n' % self._commit2)
            f.write('pick %s\n' % self._commit3)

        # We expect histedit to terminate with a nonzero exit code in this case.
        with self.assertRaises(hgrepo.HgError) as context:
            self.hg('histedit', '--commands', commands_file)
        commits = self.repo.log()
        expected_msg = (
            'Changes commited as %s. '
            'You may amend the changeset now.' % commits[0][:12]
        )
        self.assertIn(expected_msg, str(context.exception))

        # Verify the new commit stack and the histedit termination state.
        # Note that the hash of commit[0] is unpredictable because Hg gives it a
        # new hash in anticipation of the user amending it.
        self.assertEqual(2, len(commits))
        self.assertEqual(self._commit1, commits[1])

        # Make sure the working copy is in the expected state.
        self.assert_status_empty()
        all_files = set(os.listdir(self.repo.get_canonical_root()))
        self.assertSetEqual(set(['.eden', '.hg', 'first', 'second']), all_files)
