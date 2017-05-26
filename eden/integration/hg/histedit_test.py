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
from .lib.histedit_command import HisteditCommand
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
        self.assertEqual([self._commit3, self._commit2, self._commit1], commits)

        # histedit, stopping in the middle of the stack.
        histedit = HisteditCommand()
        histedit.pick(self._commit1)
        histedit.stop(self._commit2)
        histedit.pick(self._commit3)

        # We expect histedit to terminate with a nonzero exit code in this case.
        with self.assertRaises(hgrepo.HgError) as context:
            histedit.run(self)
        commits = self.repo.log()
        expected_msg = (
            'Changes commited as %s. '
            'You may amend the changeset now.' % commits[0][:12]
        )
        self.assertIn(expected_msg, str(context.exception))

        # Verify the new commit stack and the histedit termination state.
        # Note that the hash of commit[0] is unpredictable because Hg gives it a
        # new hash in anticipation of the user amending it.
        self.assertEqual(self._commit1, commits[1])
        self.assertEqual(
            ['second commit', 'first commit'], self.repo.log('{desc}\\0')
        )

        # Make sure the working copy is in the expected state.
        self.assert_status_empty()
        self.assertSetEqual(
            set(['.eden', '.hg', 'first', 'second']),
            set(os.listdir(self.repo.get_canonical_root()))
        )

        self.hg('histedit', '--continue')
        self.assertEqual(
            ['third commit', 'second commit', 'first commit'],
            self.repo.log('{desc}\\0')
        )
        self.assert_status_empty()
        self.assertSetEqual(
            set(['.eden', '.hg', 'first', 'second', 'third']),
            set(os.listdir(self.repo.get_canonical_root()))
        )

    def test_reordering_commits_without_merge_conflicts(self):
        self.assertEqual(
            ['third commit', 'second commit', 'first commit'],
            self.repo.log('{desc}\\0')
        )

        # histedit, reordering the stack in a conflict-free way.
        histedit = HisteditCommand()
        histedit.pick(self._commit2)
        histedit.pick(self._commit3)
        histedit.pick(self._commit1)
        histedit.run(self)

        self.assertEqual(
            ['first commit', 'third commit', 'second commit'],
            self.repo.log('{desc}\\0')
        )
        self.assert_status_empty()
        self.assertSetEqual(
            set(['.eden', '.hg', 'first', 'second', 'third']),
            set(os.listdir(self.repo.get_canonical_root()))
        )

    def test_drop_commit_without_merge_conflicts(self):
        self.assertEqual(
            ['third commit', 'second commit', 'first commit'],
            self.repo.log('{desc}\\0')
        )

        # histedit, reordering the stack in a conflict-free way.
        histedit = HisteditCommand()
        histedit.pick(self._commit1)
        histedit.drop(self._commit2)
        histedit.pick(self._commit3)
        histedit.run(self)

        self.assertEqual(
            ['third commit', 'first commit'], self.repo.log('{desc}\\0')
        )
        self.assert_status_empty()
        self.assertSetEqual(
            set(['.eden', '.hg', 'first', 'third']),
            set(os.listdir(self.repo.get_canonical_root()))
        )

    def test_roll_two_commits_into_parent(self):
        self.assertEqual(
            ['third commit', 'second commit', 'first commit'],
            self.repo.log('{desc}\\0')
        )

        # histedit, reordering the stack in a conflict-free way.
        histedit = HisteditCommand()
        histedit.pick(self._commit1)
        histedit.roll(self._commit2)
        histedit.roll(self._commit3)
        histedit.run(self)

        self.assertEqual(
            ['first commit'], self.repo.log('{desc}\\0')
        )
        self.assert_status_empty()
        self.assertSetEqual(
            set(['.eden', '.hg', 'first', 'second', 'third']),
            set(os.listdir(self.repo.get_canonical_root()))
        )
