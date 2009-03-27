import unittest

from mercurial import hg
from mercurial import node
from mercurial import ui

import test_util


class TestBasicRepoLayout(test_util.TestBase):
    def test_fresh_fetch_single_rev(self):
        repo = self._load_fixture_and_fetch('single_rev.svndump')
        self.assertEqual(node.hex(repo['tip'].node()),
                         '434ed487136c1b47c1e8f952edb4dc5a8e6328df')
        self.assertEqual(repo['tip'].extra()['convert_revision'],
                         'svn:df2126f7-00ab-4d49-b42c-7e981dde0bcf/trunk@2')
        self.assertEqual(repo['tip'], repo[0])

    def test_fresh_fetch_two_revs(self):
        repo = self._load_fixture_and_fetch('two_revs.svndump')
        # TODO there must be a better way than repo[0] for this check
        self.assertEqual(node.hex(repo[0].node()),
                         '434ed487136c1b47c1e8f952edb4dc5a8e6328df')
        self.assertEqual(node.hex(repo['tip'].node()),
                         'c95251e0dd04697deee99b79cc407d7db76e6a5f')
        self.assertEqual(repo['tip'], repo[1])

    def test_branches(self):
        repo = self._load_fixture_and_fetch('simple_branch.svndump')
        # TODO there must be a better way than repo[0] for this check
        self.assertEqual(node.hex(repo[0].node()),
                         '434ed487136c1b47c1e8f952edb4dc5a8e6328df')
        self.assertEqual(node.hex(repo['tip'].node()),
                         'f1ff5b860f5dbb9a59ad0921a79da77f10f25109')
        self.assertEqual(len(repo['tip'].parents()), 1)
        self.assertEqual(repo['tip'].parents()[0], repo['default'])
        self.assertEqual(repo['tip'].extra()['convert_revision'],
                         'svn:df2126f7-00ab-4d49-b42c-7e981dde0bcf/branches/the_branch@4')
        self.assertEqual(repo['default'].extra()['convert_revision'],
                         'svn:df2126f7-00ab-4d49-b42c-7e981dde0bcf/trunk@3')
        self.assertEqual(len(repo.heads()), 1)

    def test_two_branches_with_heads(self):
        repo = self._load_fixture_and_fetch('two_heads.svndump')
        # TODO there must be a better way than repo[0] for this check
        self.assertEqual(node.hex(repo[0].node()),
                         '434ed487136c1b47c1e8f952edb4dc5a8e6328df')
        self.assertEqual(node.hex(repo['tip'].node()),
                         '1083037b18d85cd84fa211c5adbaeff0fea2cd9f')
        self.assertEqual(node.hex(repo['the_branch'].node()),
                         '4e256962fc5df545e2e0a51d0d1dc61c469127e6')
        self.assertEqual(node.hex(repo['the_branch'].parents()[0].node()),
                         'f1ff5b860f5dbb9a59ad0921a79da77f10f25109')
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
                         '434ed487136c1b47c1e8f952edb4dc5a8e6328df')
        self.assertEqual(node.hex(repo['tip'].node()),
                         'b7bdc73041b1852563deb1ef3f4153c2fe4484f2')
        self.assertEqual(node.hex(repo['the_branch'].node()),
                         '4e256962fc5df545e2e0a51d0d1dc61c469127e6')
        self.assertEqual(node.hex(repo['the_branch'].parents()[0].node()),
                         'f1ff5b860f5dbb9a59ad0921a79da77f10f25109')
        self.assertEqual(len(repo['tip'].parents()), 1)
        self.assertEqual(repo['tip'], repo['default'])
        self.assertEqual(len(repo.heads()), 2)

    def test_file_mixed_with_branches(self):
        repo = self._load_fixture_and_fetch('file_mixed_with_branches.svndump')
        self.assertEqual(node.hex(repo['default'].node()),
                         '434ed487136c1b47c1e8f952edb4dc5a8e6328df')
        assert 'README' not in repo
        self.assertEqual(repo['tip'].branch(),
                         '../branches')


    def test_files_copied_from_outside_btt(self):
        repo = self._load_fixture_and_fetch(
            'test_files_copied_from_outside_btt.svndump')
        self.assertEqual(node.hex(repo['tip'].node()),
                         '3c78170e30ddd35f2c32faa0d8646ab75bba4f73')
        self.assertEqual(len(repo.changelog), 3)

    def test_file_renamed_in_from_outside_btt(self):
        repo = self._load_fixture_and_fetch(
                    'file_renamed_in_from_outside_btt.svndump')
        self.assert_('LICENSE.file' in repo['default'])

    def test_renamed_dir_in_from_outside_btt_not_repo_root(self):
        repo = self._load_fixture_and_fetch(
                    'fetch_missing_files_subdir.svndump', subdir='foo')
        self.assertEqual(node.hex(repo['tip'].node()),
                         '269dcdd4361b2847e9f4288d4500e55d35df1f52')
        self.assert_('bar/alpha' in repo['tip'])
        self.assert_('foo' in repo['tip'])
        self.assert_('bar/alpha' not in repo['tip'].parents()[0])
        self.assert_('foo' in repo['tip'].parents()[0])

    def test_oldest_not_trunk_and_tag_vendor_branch(self):
        repo = self._load_fixture_and_fetch(
            'tagged_vendor_and_oldest_not_trunk.svndump')
        self.assertEqual(node.hex(repo['oldest'].node()),
                         '926671740dec045077ab20f110c1595f935334fa')
        self.assertEqual(repo['tip'].parents()[0].parents()[0],
                         repo['oldest'])
        self.assertEqual(node.hex(repo['tip'].node()),
                         '1a6c3f30911d57abb67c257ec0df3e7bc44786f7')

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

    def test_fetch_when_trunk_has_no_files(self, stupid=False):
        repo = self._load_fixture_and_fetch('file_not_in_trunk_root.svndump', stupid=stupid)
        print repo['tip'].branch()
        print repo['tip']
        print repo['tip'].files()
        self.assertEqual(repo['tip'].branch(), 'default')

    def test_fetch_when_trunk_has_no_files_stupid(self):
        self.test_fetch_when_trunk_has_no_files(stupid=True)

class TestStupidPull(test_util.TestBase):
    def test_stupid(self):
        repo = test_util.load_fixture_and_fetch('two_heads.svndump',
                                                self.repo_path,
                                                self.wc_path,
                                                True)
        # TODO there must be a better way than repo[0] for this check
        self.assertEqual(node.hex(repo[0].node()),
                         '434ed487136c1b47c1e8f952edb4dc5a8e6328df')
        self.assertEqual(node.hex(repo['tip'].node()),
                         '1083037b18d85cd84fa211c5adbaeff0fea2cd9f')
        self.assertEqual(node.hex(repo['the_branch'].node()),
                         '4e256962fc5df545e2e0a51d0d1dc61c469127e6')
        self.assertEqual(repo['the_branch'].extra()['convert_revision'],
                         'svn:df2126f7-00ab-4d49-b42c-7e981dde0bcf/branches/the_branch@5')
        self.assertEqual(node.hex(repo['the_branch'].parents()[0].node()),
                         'f1ff5b860f5dbb9a59ad0921a79da77f10f25109')
        self.assertEqual(len(repo['tip'].parents()), 1)
        self.assertEqual(repo['default'].extra()['convert_revision'],
                         'svn:df2126f7-00ab-4d49-b42c-7e981dde0bcf/trunk@6')
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
                         '926671740dec045077ab20f110c1595f935334fa')
        self.assertEqual(repo['tip'].parents()[0].parents()[0],
                         repo['oldest'])
        self.assertEqual(node.hex(repo['tip'].node()),
                         '1a6c3f30911d57abb67c257ec0df3e7bc44786f7')

def suite():
    all = [unittest.TestLoader().loadTestsFromTestCase(TestBasicRepoLayout),
           unittest.TestLoader().loadTestsFromTestCase(TestStupidPull),
          ]
    return unittest.TestSuite(all)
