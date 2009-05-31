import unittest

from mercurial import hg
from mercurial import node
from mercurial import ui

import test_util

from hgsubversion import svnrepo

class TestTags(test_util.TestBase):
    def _load_fixture_and_fetch(self, fixture_name, stupid=False):
        return test_util.load_fixture_and_fetch(fixture_name, self.repo_path,
                                                self.wc_path, stupid=stupid)

    def test_tags(self, stupid=False):
        repo = self._load_fixture_and_fetch('basic_tag_tests.svndump',
                                            stupid=stupid)
        self.assertEqual(sorted(repo.tags()), ['copied_tag', 'tag_r3', 'tip'])
        self.assertEqual(repo['tag_r3'], repo['copied_tag'])
        self.assertEqual(repo['tag_r3'].rev(), 1)

    def test_tags_stupid(self):
        self.test_tags(stupid=True)

    def test_remove_tag(self, stupid=False):
        repo = self._load_fixture_and_fetch('remove_tag_test.svndump',
                                            stupid=stupid)
        self.assertEqual(repo['tag_r3'].rev(), 1)
        self.assert_('copied_tag' not in repo.tags())

    def test_remove_tag_stupid(self):
        self.test_remove_tag(stupid=True)

    def test_rename_tag(self, stupid=False):
        repo = self._load_fixture_and_fetch('rename_tag_test.svndump',
                                            stupid=stupid)
        self.assertEqual(repo['tag_r3'], repo['other_tag_r3'])
        self.assert_('copied_tag' not in repo.tags())

    def test_rename_tag_stupid(self):
        self.test_rename_tag(stupid=True)

    def test_branch_from_tag(self, stupid=False):
        repo = self._load_fixture_and_fetch('branch_from_tag.svndump',
                                            stupid=stupid)
        self.assert_('branch_from_tag' in repo.branchtags())
        self.assertEqual(repo[1], repo['tag_r3'])
        self.assertEqual(repo['branch_from_tag'].parents()[0], repo['copied_tag'])

    def test_branch_from_tag_stupid(self):
        self.test_branch_from_tag(stupid=True)

    def test_tag_by_renaming_branch(self, stupid=False):
        repo = self._load_fixture_and_fetch('tag_by_rename_branch.svndump',
                                            stupid=stupid)
        branches = set(repo[h] for h in repo.heads(closed=False))
        self.assert_('dummy' not in branches)
        self.assertEqual(repo['dummy'], repo['tip'].parents()[0].parents()[0])
        extra = repo['tip'].extra().copy()
        extra.pop('convert_revision', None)
        self.assertEqual(extra, {'branch': 'dummy', 'close': '1'})

    def test_tag_by_renaming_branch_stupid(self):
        self.test_tag_by_renaming_branch(stupid=True)

    def test_deletion_of_tag_on_trunk_after_branching(self):
        repo = self._load_fixture_and_fetch('tag_deletion_tag_branch.svndump')
        branches = set(repo[h].extra()['branch'] for h in repo.heads(closed=False))
        self.assertEqual(branches, set(['default', 'from_2', ]))
        self.assertEqual(
            repo.tags(),
            {'tip': 'g\xdd\xcd\x93\x03g\x1e\x7f\xa6-[V%\x99\x07\xd3\x9d>(\x94',
             'new_tag': '=\xb8^\xb5\x18\xa9M\xdb\xf9\xb62Z\xa0\xb5R6+\xfe6.'})


def suite():
    return unittest.TestLoader().loadTestsFromTestCase(TestTags)
