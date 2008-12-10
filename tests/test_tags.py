import unittest

from mercurial import hg
from mercurial import node
from mercurial import ui

import test_util

import svncommand
import tag_repo

class TestTags(test_util.TestBase):
    def _load_fixture_and_fetch(self, fixture_name, stupid=False):
        return test_util.load_fixture_and_fetch(fixture_name, self.repo_path,
                                                self.wc_path, stupid=stupid)

    def getrepo(self):
        ui_ = ui.ui()
        repo = hg.repository(ui_, self.wc_path)
        repo.__class__ = tag_repo.generate_repo_class(ui_, repo)
        return repo

    def _test_tag_revision_info(self, repo):
        self.assertEqual(node.hex(repo[0].node()),
                         'a47d0ce778660a91c31bf2c21c448e9ee296ac90')
        self.assertEqual(node.hex(repo['tip'].node()),
                         'bf3767835b3b32ecc775a298c2fa27134dd91c11')
        self.assertEqual(repo['tip'], repo[1])

    def test_tags(self, stupid=False):
        repo = self._load_fixture_and_fetch('basic_tag_tests.svndump',
                                            stupid=stupid)
        self._test_tag_revision_info(repo)
        repo = self.getrepo()
        self.assertEqual(repo['tip'].node(), repo['tag/tag_r3'].node())
        self.assertEqual(repo['tip'].node(), repo['tag/copied_tag'].node())

    def test_tags_stupid(self):
        self.test_tags(stupid=True)

    def test_remove_tag(self, stupid=False):
        repo = self._load_fixture_and_fetch('remove_tag_test.svndump',
                                            stupid=stupid)
        self._test_tag_revision_info(repo)
        repo = self.getrepo()
        self.assertEqual(repo['tip'].node(), repo['tag/tag_r3'].node())
        self.assert_('tag/copied_tag' not in repo.tags())

    def test_remove_tag_stupid(self):
        self.test_remove_tag(stupid=True)

    def test_rename_tag(self, stupid=False):
        repo = self._load_fixture_and_fetch('rename_tag_test.svndump',
                                            stupid=stupid)
        self._test_tag_revision_info(repo)
        repo = self.getrepo()
        self.assertEqual(repo['tip'].node(), repo['tag/tag_r3'].node())
        self.assertEqual(repo['tip'].node(), repo['tag/other_tag_r3'].node())
        self.assert_('tag/copied_tag' not in repo.tags())

    def test_rename_tag_stupid(self):
        self.test_rename_tag(stupid=True)

    def test_branch_from_tag(self, stupid=False):
        repo = self._load_fixture_and_fetch('branch_from_tag.svndump',
                                            stupid=stupid)
        repo = self.getrepo()
        self.assertEqual(repo['tip'].node(), repo['branch_from_tag'].node())
        self.assertEqual(repo[1].node(), repo['tag/tag_r3'].node())
        self.assertEqual(repo['branch_from_tag'].parents()[0].node(),
                         repo['tag/copied_tag'].node())

    def test_branch_from_tag_stupid(self):
        self.test_branch_from_tag(stupid=True)

    def test_tag_by_renaming_branch(self, stupid=False):
        repo = self._load_fixture_and_fetch('tag_by_rename_branch.svndump',
                                            stupid=stupid)
        repo = self.getrepo()
        self.assertEqual(repo['tip'], repo['closed-branches'])
        self.assertEqual(node.hex(repo['tip'].node()),
                         'dd2dccc3180631192f058468ec7215899223a6d8')
        taggedrev = repo['tip'].parents()[0]
        self.assertEqual(node.hex(taggedrev.node()),
                         '68f5f7d82b00a2efe3aca28b615ebab98235d55f')
        self.assertEqual(node.hex(repo['tag/dummy'].node()),
                         '68f5f7d82b00a2efe3aca28b615ebab98235d55f')

    def test_tag_by_renaming_branch_stupid(self):
        self.test_tag_by_renaming_branch(stupid=True)

def suite():
    return unittest.TestLoader().loadTestsFromTestCase(TestTags)
