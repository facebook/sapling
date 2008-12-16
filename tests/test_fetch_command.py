import unittest

from mercurial import hg
from mercurial import ui
from mercurial import node

import test_util


class TestBasicRepoLayout(test_util.TestBase):
    def test_fresh_fetch_single_rev(self):
        repo = self._load_fixture_and_fetch('single_rev.svndump')
        self.assertEqual(node.hex(repo['tip'].node()),
                         'a47d0ce778660a91c31bf2c21c448e9ee296ac90')
        self.assertEqual(repo['tip'], repo[0])

    def test_fresh_fetch_two_revs(self):
        repo = self._load_fixture_and_fetch('two_revs.svndump')
        # TODO there must be a better way than repo[0] for this check
        self.assertEqual(node.hex(repo[0].node()),
                         'a47d0ce778660a91c31bf2c21c448e9ee296ac90')
        self.assertEqual(node.hex(repo['tip'].node()),
                         'bf3767835b3b32ecc775a298c2fa27134dd91c11')
        self.assertEqual(repo['tip'], repo[1])

    def test_branches(self):
        repo = self._load_fixture_and_fetch('simple_branch.svndump')
        # TODO there must be a better way than repo[0] for this check
        self.assertEqual(node.hex(repo[0].node()),
                         'a47d0ce778660a91c31bf2c21c448e9ee296ac90')
        self.assertEqual(node.hex(repo['tip'].node()),
                         '9dfb0a19494f45c36e22f3c6d1b21d80638a7f6e')
        self.assertEqual(len(repo['tip'].parents()), 1)
        self.assertEqual(repo['tip'].parents()[0], repo['default'])
        self.assertEqual(len(repo.heads()), 1)

    def test_two_branches_with_heads(self):
        repo = self._load_fixture_and_fetch('two_heads.svndump')
        # TODO there must be a better way than repo[0] for this check
        self.assertEqual(node.hex(repo[0].node()),
                         'a47d0ce778660a91c31bf2c21c448e9ee296ac90')
        self.assertEqual(node.hex(repo['tip'].node()),
                         'a595c77cfcaa3d1ba9e04b2c55c68bc6bf2b0fbf')
        self.assertEqual(node.hex(repo['the_branch'].node()),
                         '8ccaba5f0eae124487e413abd904a013f7f6fdeb')
        self.assertEqual(node.hex(repo['the_branch'].parents()[0].node()),
                         '9dfb0a19494f45c36e22f3c6d1b21d80638a7f6e')
        self.assertEqual(len(repo['tip'].parents()), 1)
        self.assertEqual(repo['tip'], repo['default'])
        self.assertEqual(len(repo.heads()), 2)

    def test_many_special_cases_replay(self):
        repo = self._load_fixture_and_fetch('many_special_cases.svndump')
        # TODO there must be a better way than repo[0] for this check
        self._many_special_cases_checks(repo)


    def test_many_special_cases_diff(self):
        repo = self._load_fixture_and_fetch('many_special_cases.svndump',
                                            stupid=True)
        # TODO there must be a better way than repo[0] for this check
        self._many_special_cases_checks(repo)

    def _many_special_cases_checks(self, repo):
        self.assertEqual(node.hex(repo[0].node()),
                         'a47d0ce778660a91c31bf2c21c448e9ee296ac90')
        self.assertEqual(node.hex(repo['tip'].node()),
                         '179fb7d9bc77eef78288661f0430e0c1dff56b6f')
        self.assertEqual(node.hex(repo['the_branch'].node()),
                         '8ccaba5f0eae124487e413abd904a013f7f6fdeb')
        self.assertEqual(node.hex(repo['the_branch'].parents()[0].node()),
                         '9dfb0a19494f45c36e22f3c6d1b21d80638a7f6e')
        self.assertEqual(len(repo['tip'].parents()), 1)
        self.assertEqual(repo['tip'], repo['default'])
        self.assertEqual(len(repo.heads()), 2)

    def test_file_mixed_with_branches(self):
        repo = self._load_fixture_and_fetch('file_mixed_with_branches.svndump')
        self.assertEqual(node.hex(repo['tip'].node()),
                         'a47d0ce778660a91c31bf2c21c448e9ee296ac90')
        assert 'README' not in repo

    def test_files_copied_from_outside_btt(self):
        repo = self._load_fixture_and_fetch(
            'test_files_copied_from_outside_btt.svndump')
        self.assertEqual(node.hex(repo['tip'].node()),
                         'c4e669a763a70f751c71d4534a34a65f398d71d4')
        self.assertEqual(len(repo.changelog), 2)

    def test_file_renamed_in_from_outside_btt(self):
        repo = self._load_fixture_and_fetch(
                    'file_renamed_in_from_outside_btt.svndump')
        self.assert_('LICENSE.file' in repo['tip'])

    def test_renamed_dir_in_from_outside_btt_not_repo_root(self):
        repo = self._load_fixture_and_fetch(
                    'fetch_missing_files_subdir.svndump', subdir='foo')
        self.assertEqual(node.hex(repo['tip'].node()),
                         '2fae2544a5858d0bc6c04976683b3dcc0416d6e3')
        self.assert_('bar/alpha' in repo['tip'])
        self.assert_('foo' in repo['tip'])
        self.assert_('bar/alpha' not in repo['tip'].parents()[0])
        self.assert_('foo' in repo['tip'].parents()[0])

    def test_oldest_not_trunk_and_tag_vendor_branch(self):
        repo = self._load_fixture_and_fetch(
            'tagged_vendor_and_oldest_not_trunk.svndump')
        self.assertEqual(node.hex(repo['oldest'].node()),
                         'd73002bcdeffe389a8df81ee43303d36e79e8ca4')
        self.assertEqual(repo['tip'].parents()[0].parents()[0],
                         repo['oldest'])
        self.assertEqual(node.hex(repo['tip'].node()),
                         '9cf09e6ff7fa938188c3bcc9dd87abd7842c080c')

    def test_propedit_with_nothing_else(self, stupid=False):
        repo = self._load_fixture_and_fetch('branch_prop_edit.svndump',
                                            stupid=stupid)
        self.assertEqual(repo['tip'].description(), 'Commit bogus propchange.')
        self.assertEqual(repo['tip'].branch(), 'dev_branch')

    def test_propedit_with_nothing_else_stupid(self):
        self.test_propedit_with_nothing_else(stupid=True)

    def test_entry_deletion(self, stupid=False):
        repo = self._load_fixture_and_fetch('delentries.svndump',
                                            stupid=stupid)
        files = list(sorted(repo['tip'].manifest()))
        self.assertEqual(['aa', 'd1/c', 'd1/d2prefix'], files)

    def test_entry_deletion_stupid(self):
        self.test_entry_deletion(stupid=True)


class TestStupidPull(test_util.TestBase):
    def test_stupid(self):
        repo = test_util.load_fixture_and_fetch('two_heads.svndump',
                                                self.repo_path,
                                                self.wc_path,
                                                True)
        # TODO there must be a better way than repo[0] for this check
        self.assertEqual(node.hex(repo[0].node()),
                         'a47d0ce778660a91c31bf2c21c448e9ee296ac90')
        self.assertEqual(node.hex(repo['tip'].node()),
                         'a595c77cfcaa3d1ba9e04b2c55c68bc6bf2b0fbf')
        self.assertEqual(node.hex(repo['the_branch'].node()),
                         '8ccaba5f0eae124487e413abd904a013f7f6fdeb')
        self.assertEqual(node.hex(repo['the_branch'].parents()[0].node()),
                         '9dfb0a19494f45c36e22f3c6d1b21d80638a7f6e')
        self.assertEqual(len(repo['tip'].parents()), 1)
        self.assertEqual(repo['tip'], repo['default'])
        self.assertEqual(len(repo.heads()), 2)

    def test_oldest_not_trunk_and_tag_vendor_branch(self):
        repo = test_util.load_fixture_and_fetch(
            'tagged_vendor_and_oldest_not_trunk.svndump',
            self.repo_path,
            self.wc_path,
            True)
        repo = hg.repository(ui.ui(), self.wc_path)
        self.assertEqual(node.hex(repo['oldest'].node()),
                         'd73002bcdeffe389a8df81ee43303d36e79e8ca4')
        self.assertEqual(repo['tip'].parents()[0].parents()[0],
                         repo['oldest'])
        self.assertEqual(node.hex(repo['tip'].node()),
                         '9cf09e6ff7fa938188c3bcc9dd87abd7842c080c')

def suite():
    all = [unittest.TestLoader().loadTestsFromTestCase(TestBasicRepoLayout),
           unittest.TestLoader().loadTestsFromTestCase(TestStupidPull),
          ]
    return unittest.TestSuite(all)
