"""Tests for author maps and file maps.
"""
import test_util

import os
import unittest

from mercurial import commands
from mercurial import hg
from mercurial import node
from mercurial import util as hgutil

from hgsubversion import maps
from hgsubversion import svncommands
from hgsubversion import util

class MapTests(test_util.TestBase):
    @property
    def authors(self):
        return os.path.join(self.tmpdir, 'authormap')

    @property
    def filemap(self):
        return os.path.join(self.tmpdir, 'filemap')

    @property
    def branchmap(self):
        return os.path.join(self.tmpdir, 'branchmap')
	
    @property
    def tagmap(self):
        return os.path.join(self.tmpdir, 'tagmap')

    def test_author_map(self, stupid=False):
        test_util.load_svndump_fixture(self.repo_path, 'replace_trunk_with_branch.svndump')
        authormap = open(self.authors, 'w')
        authormap.write('Augie=Augie Fackler <durin42@gmail.com> # stuffy\n')
        authormap.write("Augie Fackler <durin42@gmail.com>\n")
        authormap.close()
        ui = self.ui(stupid)
        ui.setconfig('hgsubversion', 'authormap', self.authors)
        commands.clone(ui, test_util.fileurl(self.repo_path),
                       self.wc_path, authors=self.authors)
        self.assertEqual(self.repo[0].user(),
                         'Augie Fackler <durin42@gmail.com>')
        self.assertEqual(self.repo['tip'].user(),
                        'evil@5b65bade-98f3-4993-a01f-b7a6710da339')

    def test_author_map_stupid(self):
        self.test_author_map(True)

    def test_author_map_closing_author(self, stupid=False):
        test_util.load_svndump_fixture(self.repo_path, 'replace_trunk_with_branch.svndump')
        authormap = open(self.authors, 'w')
        authormap.write("evil=Testy <test@test>")
        authormap.close()
        ui = self.ui(stupid)
        ui.setconfig('hgsubversion', 'authormap', self.authors)
        commands.clone(ui, test_util.fileurl(self.repo_path),
                       self.wc_path, authors=self.authors)
        self.assertEqual(self.repo[0].user(),
                         'Augie@5b65bade-98f3-4993-a01f-b7a6710da339')
        self.assertEqual(self.repo['tip'].user(),
                        'Testy <test@test>')

    def test_author_map_closing_author_stupid(self):
        self.test_author_map_closing_author(True)

    def test_author_map_no_author(self, stupid=False):
        self._load_fixture_and_fetch('no-author.svndump', stupid=stupid)
        users = set(self.repo[r].user() for r in self.repo)
        expected_users = ['(no author)@%s' % self.repo.svnmeta().uuid]
        self.assertEqual(sorted(users), expected_users)
        test_util.rmtree(self.wc_path)

        authormap = open(self.authors, 'w')
        authormap.write("(no author)=Testy <test@example.com>")
        authormap.close()
        ui = self.ui(stupid)
        ui.setconfig('hgsubversion', 'authormap', self.authors)
        commands.clone(ui, test_util.fileurl(self.repo_path),
                       self.wc_path, authors=self.authors)
        users = set(self.repo[r].user() for r in self.repo)
        expected_users = ['Testy <test@example.com>']
        self.assertEqual(sorted(users), expected_users)

    def test_author_map_no_author_stupid(self):
        self.test_author_map_no_author(True)

    def test_author_map_no_overwrite(self):
        cwd = os.path.dirname(__file__)
        orig = os.path.join(cwd, 'fixtures', 'author-map-test.txt')
        new = open(self.authors, 'w')
        new.write(open(orig).read())
        new.close()
        test = maps.AuthorMap(self.ui(), self.authors)
        fromself = set(test)
        test.load(orig)
        all = set(test)
        self.assertEqual(fromself.symmetric_difference(all), set())

    def test_file_map(self, stupid=False):
        test_util.load_svndump_fixture(self.repo_path, 'replace_trunk_with_branch.svndump')
        filemap = open(self.filemap, 'w')
        filemap.write("include alpha\n")
        filemap.close()
        ui = self.ui(stupid)
        ui.setconfig('hgsubversion', 'filemap', self.filemap)
        commands.clone(ui, test_util.fileurl(self.repo_path),
                       self.wc_path, filemap=self.filemap)
        self.assertEqual(node.hex(self.repo[0].node()), '88e2c7492d83e4bf30fbb2dcbf6aa24d60ac688d')
        self.assertEqual(node.hex(self.repo['default'].node()), 'e524296152246b3837fe9503c83b727075835155')

    def test_file_map_stupid(self):
        self.test_file_map(True)

    def test_file_map_exclude(self, stupid=False):
        test_util.load_svndump_fixture(self.repo_path, 'replace_trunk_with_branch.svndump')
        filemap = open(self.filemap, 'w')
        filemap.write("exclude alpha\n")
        filemap.close()
        ui = self.ui(stupid)
        ui.setconfig('hgsubversion', 'filemap', self.filemap)
        commands.clone(ui, test_util.fileurl(self.repo_path),
                       self.wc_path, filemap=self.filemap)
        self.assertEqual(node.hex(self.repo[0].node()), '2c48f3525926ab6c8b8424bcf5eb34b149b61841')
        self.assertEqual(node.hex(self.repo['default'].node()), 'b37a3c0297b71f989064d9b545b5a478bbed7cc1')

    def test_file_map_exclude_stupid(self):
        self.test_file_map_exclude(True)

    def test_branchmap(self, stupid=False):
        test_util.load_svndump_fixture(self.repo_path, 'branchmap.svndump')
        branchmap = open(self.branchmap, 'w')
        branchmap.write("badname = good-name # stuffy\n")
        branchmap.write("feature = default\n")
        branchmap.close()
        ui = self.ui(stupid)
        ui.setconfig('hgsubversion', 'branchmap', self.branchmap)
        commands.clone(ui, test_util.fileurl(self.repo_path),
                       self.wc_path, branchmap=self.branchmap)
        branches = set(self.repo[i].branch() for i in self.repo)
        self.assert_('badname' not in branches)
        self.assert_('good-name' in branches)
        self.assertEquals(self.repo[2].branch(), 'default')

    def test_branchmap_stupid(self):
        self.test_branchmap(True)

    def test_branchmap_tagging(self, stupid=False):
        '''test tagging a renamed branch, which used to raise an exception'''
        test_util.load_svndump_fixture(self.repo_path, 'commit-to-tag.svndump')
        branchmap = open(self.branchmap, 'w')
        branchmap.write("magic = art\n")
        branchmap.close()
        ui = self.ui(stupid)
        ui.setconfig('hgsubversion', 'branchmap', self.branchmap)
        commands.clone(ui, test_util.fileurl(self.repo_path),
                       self.wc_path, branchmap=self.branchmap)
        branches = set(self.repo[i].branch() for i in self.repo)
        self.assertEquals(sorted(branches), ['art', 'closeme'])

    def test_branchmap_tagging_stupid(self):
        self.test_branchmap_tagging(True)

    def test_branchmap_empty_commit(self, stupid=False):
        '''test mapping an empty commit on a renamed branch'''
        test_util.load_svndump_fixture(self.repo_path, 'propset-branch.svndump')
        branchmap = open(self.branchmap, 'w')
        branchmap.write("the-branch = bob\n")
        branchmap.close()
        ui = self.ui(stupid)
        ui.setconfig('hgsubversion', 'branchmap', self.branchmap)
        commands.clone(ui, test_util.fileurl(self.repo_path),
                       self.wc_path, branchmap=self.branchmap)
        branches = set(self.repo[i].branch() for i in self.repo)
        self.assertEquals(sorted(branches), ['bob', 'default'])

    def test_branchmap_empty_commit_stupid(self):
        '''test mapping an empty commit on a renamed branch (stupid)'''
        self.test_branchmap_empty_commit(True)

    def test_branchmap_combine(self, stupid=False):
        '''test combining two branches, but retaining heads'''
        test_util.load_svndump_fixture(self.repo_path, 'branchmap.svndump')
        branchmap = open(self.branchmap, 'w')
        branchmap.write("badname = default\n")
        branchmap.write("feature = default\n")
        branchmap.close()
        ui = self.ui(stupid)
        ui.setconfig('hgsubversion', 'branchmap', self.branchmap)
        commands.clone(ui, test_util.fileurl(self.repo_path),
                       self.wc_path, branchmap=self.branchmap)
        branches = set(self.repo[i].branch() for i in self.repo)
        self.assertEquals(sorted(branches), ['default'])
        self.assertEquals(len(self.repo.heads()), 2)
        self.assertEquals(len(self.repo.branchheads('default')), 2)

        # test that the mapping does not affect branch info
        branches = self.repo.svnmeta().branches
        self.assertEquals(sorted(branches.keys()),
                          [None, 'badname', 'feature'])

    def test_branchmap_combine_stupid(self):
        '''test combining two branches, but retaining heads (stupid)'''
        self.test_branchmap_combine(True)

    def test_branchmap_rebuildmeta(self, stupid=False):
        '''test rebuildmeta on a branchmapped clone'''
        test_util.load_svndump_fixture(self.repo_path, 'branchmap.svndump')
        branchmap = open(self.branchmap, 'w')
        branchmap.write("badname = dit\n")
        branchmap.write("feature = dah\n")
        branchmap.close()
        ui = self.ui(stupid)
        ui.setconfig('hgsubversion', 'branchmap', self.branchmap)
        commands.clone(ui, test_util.fileurl(self.repo_path),
                       self.wc_path, branchmap=self.branchmap)
        originfo = self.repo.svnmeta().branches

        # clone & rebuild
        ui = self.ui(stupid)
        src, dest = hg.clone(ui, self.wc_path, self.wc_path + '_clone',
                             update=False)
        svncommands.rebuildmeta(ui, dest,
                                args=[test_util.fileurl(self.repo_path)])

        # just check the keys; assume the contents are unaffected by the branch
        # map and thus properly tested by other tests
        self.assertEquals(sorted(src.svnmeta().branches),
                          sorted(dest.svnmeta().branches))

    def test_branchmap_rebuildmeta_stupid(self):
        '''test rebuildmeta on a branchmapped clone (stupid)'''
        self.test_branchmap_rebuildmeta(True)

    def test_branchmap_verify(self, stupid=False):
        '''test verify on a branchmapped clone'''
        test_util.load_svndump_fixture(self.repo_path, 'branchmap.svndump')
        branchmap = open(self.branchmap, 'w')
        branchmap.write("badname = dit\n")
        branchmap.write("feature = dah\n")
        branchmap.close()
        ui = self.ui(stupid)
        ui.setconfig('hgsubversion', 'branchmap', self.branchmap)
        commands.clone(ui, test_util.fileurl(self.repo_path),
                       self.wc_path, branchmap=self.branchmap)
        repo = self.repo

        for r in repo:
            self.assertEquals(svncommands.verify(ui, repo, rev=r), 0)

    def test_branchmap_verify_stupid(self):
        '''test verify on a branchmapped clone (stupid)'''
        self.test_branchmap_verify(True)

    def test_branchmap_no_replacement(self):
        '''
        test that empty mappings are rejected

        Empty mappings are lines like 'this ='. The most sensible thing to do
        is to not convert the 'this' branches. Until we can do that, we settle
        with aborting.
        '''
        test_util.load_svndump_fixture(self.repo_path, 'propset-branch.svndump')
        branchmap = open(self.branchmap, 'w')
        branchmap.write("closeme =\n")
        branchmap.close()
        self.assertRaises(hgutil.Abort,
                          maps.BranchMap, self.ui(), self.branchmap)

    def test_tagmap(self, stupid=False):
        test_util.load_svndump_fixture(self.repo_path,
                                       'basic_tag_tests.svndump')
        tagmap = open(self.tagmap, 'w')
        tagmap.write("tag_r3 = 3.x # stuffy\n")
        tagmap.write("copied_tag = \n")
        tagmap.close()

        ui = self.ui(stupid)
        ui.setconfig('hgsubversion', 'tagmap', self.tagmap)
        commands.clone(ui, test_util.fileurl(self.repo_path),
                       self.wc_path, tagmap=self.tagmap)
        tags = self.repo.tags()
        assert 'tag_r3' not in tags
        assert '3.x' in tags
        assert 'copied_tag' not in tags

    def test_tagmap_stupid(self):
        self.test_tagmap(True)

    def test_tagren_changed(self, stupid=False):
        test_util.load_svndump_fixture(self.repo_path,
                                       'commit-to-tag.svndump')
        tagmap = open(self.tagmap, 'w')
        tagmap.write("edit-at-create = edit-past\n")
        tagmap.write("also-edit = \n")
        tagmap.write("will-edit = edit-future\n")
        tagmap.close()

        ui = self.ui(stupid)
        ui.setconfig('hgsubversion', 'tagmap', self.tagmap)
        commands.clone(ui, test_util.fileurl(self.repo_path),
                       self.wc_path, tagmap=self.tagmap)
        tags = self.repo.tags()

    def test_tagren_changed_stupid(self):
        self.test_tagren_changed(True)

    def test_empty_log_message(self, stupid=False):
        repo = self._load_fixture_and_fetch('empty-log-message.svndump',
                                            stupid=stupid)

        self.assertEqual(repo['tip'].description(), '')

        test_util.rmtree(self.wc_path)

        ui = self.ui(stupid)
        ui.setconfig('hgsubversion', 'defaultmessage', 'blyf')
        commands.clone(ui, test_util.fileurl(self.repo_path), self.wc_path)

        self.assertEqual(self.repo['tip'].description(), 'blyf')


    def test_empty_log_message_stupid(self):
        self.test_empty_log_message(True)

def suite():
    return unittest.TestLoader().loadTestsFromTestCase(MapTests)
