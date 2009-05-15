import unittest

from mercurial import hg
from mercurial import node
from mercurial import ui

import test_util

import svnrepo

class TestTags(test_util.TestBase):
    def _load_fixture_and_fetch(self, fixture_name, stupid=False):
        return test_util.load_fixture_and_fetch(fixture_name, self.repo_path,
                                                self.wc_path, stupid=stupid)

    def _test_tag_revision_info(self, repo):
        print repo.tags()
        self.assertEqual(node.hex(repo[0].node()),
                         '434ed487136c1b47c1e8f952edb4dc5a8e6328df')
        self.assertEqual(node.hex(repo['tip'].node()),
                         'c95251e0dd04697deee99b79cc407d7db76e6a5f')
        self.assertEqual(repo['tip'], repo[1])

    def test_tags(self, stupid=False):
        repo = self._load_fixture_and_fetch('basic_tag_tests.svndump',
                                            stupid=stupid)
        self._test_tag_revision_info(repo)
        repo = self.repo
        self.assertEqual(repo['tip'].node(), repo['tag/tag_r3'].node())
        self.assertEqual(repo['tip'].node(), repo['tag/copied_tag'].node())

    def test_tags_stupid(self):
        self.test_tags(stupid=True)

    def test_remove_tag(self, stupid=False):
        repo = self._load_fixture_and_fetch('remove_tag_test.svndump',
                                            stupid=stupid)
        self._test_tag_revision_info(repo)
        repo = self.repo
        self.assertEqual(repo['tip'].node(), repo['tag/tag_r3'].node())
        self.assert_('tag/copied_tag' not in repo.tags())

    def test_remove_tag_stupid(self):
        self.test_remove_tag(stupid=True)

    def test_rename_tag(self, stupid=False):
        repo = self._load_fixture_and_fetch('rename_tag_test.svndump',
                                            stupid=stupid)
        self._test_tag_revision_info(repo)
        repo = self.repo
        self.assertEqual(repo['tip'].node(), repo['tag/tag_r3'].node())
        self.assertEqual(repo['tip'].node(), repo['tag/other_tag_r3'].node())
        self.assert_('tag/copied_tag' not in repo.tags())

    def test_rename_tag_stupid(self):
        self.test_rename_tag(stupid=True)

    def test_branch_from_tag(self, stupid=False):
        repo = self._load_fixture_and_fetch('branch_from_tag.svndump',
                                            stupid=stupid)
        repo = self.repo
        self.assertEqual(repo['tip'].node(), repo['branch_from_tag'].node())
        self.assertEqual(repo[1].node(), repo['tag/tag_r3'].node())
        self.assertEqual(repo['branch_from_tag'].parents()[0].node(),
                         repo['tag/copied_tag'].node())

    def test_branch_from_tag_stupid(self):
        self.test_branch_from_tag(stupid=True)

    def test_tag_by_renaming_branch(self, stupid=False):
        repo = self._load_fixture_and_fetch('tag_by_rename_branch.svndump',
                                            stupid=stupid)
        repo = self.repo
        self.assertEqual(repo['tip'], repo['closed-branches'])
        self.assertEqual(node.hex(repo['tip'].node()),
                         '2f0a3abe2004c0fa01f5f6074a8b5441e9c80c2a')
        taggedrev = repo['tip'].parents()[0]
        self.assertEqual(node.hex(taggedrev.node()),
                         '50c67c73267987de705ee335183c5486641e56e9')
        self.assertEqual(node.hex(repo['tag/dummy'].node()),
                         '50c67c73267987de705ee335183c5486641e56e9')

    def test_tag_by_renaming_branch_stupid(self):
        self.test_tag_by_renaming_branch(stupid=True)

def suite():
    return unittest.TestLoader().loadTestsFromTestCase(TestTags)
