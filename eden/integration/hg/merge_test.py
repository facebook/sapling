#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
class MergeTest(EdenHgTestCase):
    '''Note that Mercurial has a number of built-in merge tools:
    https://www.mercurial-scm.org/repo/hg/help/merge-tools
    '''
    def populate_backing_repo(self, repo):
        repo.write_file('foo', 'original')
        self.commit0 = repo.commit('root commit')

        repo.write_file('foo', '1')
        self.commit1 = repo.commit('commit1')
        repo.update(self.commit0)

        repo.write_file('foo', '2')
        self.commit2 = repo.commit('commit2')

    def test_merge_local(self):
        self._do_merge_and_commit(':local')
        self._verify_tip('2')

    def test_merge_other(self):
        self._do_merge_and_commit(':other')
        self._verify_tip('1')

    def test_merge_union(self):
        self._do_merge_and_commit(':union')
        self._verify_tip('21')

    def _do_merge_and_commit(self, tool):
        self.hg('merge', '--tool', tool, '-r', self.commit1)
        self.assert_status({'foo': 'M'})
        self.repo.commit('merge commit1 into commit2')
        self.assert_status_empty()

    def _verify_tip(self, expected_contents):
        files = self.repo.log(template='{files}', revset='tip')[0]
        self.assertEqual('foo', files)

        p1, p2 = self.repo.log(
            template='{p1node}\n{p2node}', revset='tip'
        )[0].split('\n')
        self.assertEqual(self.commit2, p1)
        self.assertEqual(self.commit1, p2)
        self.assertEqual(expected_contents, self.read_file('foo'))
