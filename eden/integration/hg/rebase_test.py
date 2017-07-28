#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from .lib.hg_extension_test_base import HgExtensionTestBase
from ..lib import eden_server_inspector


class RebaseTest(HgExtensionTestBase):
    def populate_backing_repo(self, repo):
        repo.mkdir('numbers')
        repo.write_file('numbers/README', 'this will have two directories')
        self._base_commit = repo.commit('commit')

        repo.mkdir('numbers/1')
        repo.write_file('numbers/1/11', '')
        self._c11 = repo.commit('c11')
        repo.write_file('numbers/1/12', '')
        self._c12 = repo.commit('c12')
        repo.write_file('numbers/1/13', '')
        self._c13 = repo.commit('c13')
        repo.write_file('numbers/1/14', '')
        self._c14 = repo.commit('c14')
        repo.write_file('numbers/1/15', '')
        self._c15 = repo.commit('c15')

        repo.update(self._base_commit)
        repo.mkdir('numbers/2')
        repo.write_file('numbers/2/21', '')
        self._c21 = repo.commit('c21')
        repo.write_file('numbers/2/22', '')
        self._c22 = repo.commit('c22')
        repo.write_file('numbers/2/23', '')
        self._c23 = repo.commit('c23')
        repo.write_file('numbers/2/24', '')
        self._c24 = repo.commit('c24')
        repo.write_file('numbers/2/25', '')
        self._c25 = repo.commit('c25')

        repo.update(self._base_commit)

    def test_rebase_commit_with_independent_folder(self):
        stdout = self.hg('rebase', '-s', self._c11, '-d', self._c25)
        expected_stdout = f'''\
rebasing 1:{self._c11[:12]} "c11"
rebasing 2:{self._c12[:12]} "c12"
rebasing 3:{self._c13[:12]} "c13"
rebasing 4:{self._c14[:12]} "c14"
rebasing 5:{self._c15[:12]} "c15"
'''
        self.assertEqual(expected_stdout, stdout)

        # Get the hash of the new head created as a result of the rebase.
        new_head = self.hg(
            'log', '-r', f'successors({self._c15})', '-T', '{node}'
        )

        # Record the pre-update inode count.
        inspector = eden_server_inspector.EdenServerInspector(self.repo.path)
        inspector.unload_inode_for_path('numbers')
        pre_update_count = inspector.get_inode_count('numbers')
        print(f'loaded inode count before `hg update`: {pre_update_count}')

        # Verify that updating to the new head that was created as a result of
        # the rebase leaves Hg in the correct state.
        self.assertEqual(1, len(self.repo.log()), msg=(
            'At the base commit, `hg log` should have only one entry.'
        ))
        self.hg('update', new_head)
        self.assertEqual(11, len(self.repo.log()), msg=(
            'The new head should include all the commits.'
        ))

        # Verify the post-update inode count.
        post_update_count = inspector.get_inode_count('numbers')
        print(f'loaded inode count after `hg update`: {post_update_count}')
        self.assertGreaterEqual(post_update_count, pre_update_count, msg=(
            'The inode count should not decrease due to `hg update`.'
        ))
        num_new_inodes = post_update_count - pre_update_count
        self.assertLessEqual(num_new_inodes, 2, msg=(
            'There should be no more than 2 new inodes as a result of the '
            'update. At the time this test was created, num_new_inodes is 0, '
            'but if we included unloaded inodes, there would be 2: one for '
            'numbers/1 and one for numbers/2.'
        ))
