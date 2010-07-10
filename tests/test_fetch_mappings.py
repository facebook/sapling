"""Tests for author maps and file maps.
"""
import os
import unittest

from mercurial import commands
from mercurial import node

import test_util

from hgsubversion import maps

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

def suite():
    return unittest.TestLoader().loadTestsFromTestCase(MapTests)
