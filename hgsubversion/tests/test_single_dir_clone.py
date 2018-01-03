import test_util

import errno
import shutil
import unittest

from mercurial import commands
from mercurial import context
from mercurial import hg
from mercurial import node
from mercurial import ui

from hgsubversion import compathacks

class TestSingleDirClone(test_util.TestBase):
    stupid_mode_tests = True

    def test_clone_single_dir_simple(self):
        repo = self._load_fixture_and_fetch('branch_from_tag.svndump',
                                            layout='single',
                                            subdir='')
        self.assertEqual(compathacks.branchset(repo),
                         set(['default']))
        self.assertEqual(sorted(repo['tip'].manifest().keys()),
                         ['branches/branch_from_tag/alpha',
                          'branches/branch_from_tag/beta',
                          'tags/copied_tag/alpha',
                          'tags/copied_tag/beta',
                          'tags/tag_r3/alpha',
                          'tags/tag_r3/beta',
                          'trunk/alpha',
                          'trunk/beta'])

    def test_auto_detect_single(self):
        repo = self._load_fixture_and_fetch('branch_from_tag.svndump',
                                            layout='auto')
        self.assertEqual(compathacks.branchset(repo),
                         set(['default', 'branch_from_tag']))
        oldmanifest = test_util.filtermanifest(repo['default'].manifest().keys())
        # remove standard layout
        shutil.rmtree(self.wc_path)
        # try again with subdir to get single dir clone
        repo = self._load_fixture_and_fetch('branch_from_tag.svndump',
                                            layout='auto',
                                            subdir='trunk')
        self.assertEqual(compathacks.branchset(repo), set(['default', ]))
        self.assertEqual(repo['default'].manifest().keys(), oldmanifest)

    def test_clone_subdir_is_file_prefix(self):
        FIXTURE = 'subdir_is_file_prefix.svndump'
        repo = self._load_fixture_and_fetch(FIXTURE,
                                            layout='single',
                                            subdir=test_util.subdir[FIXTURE])
        self.assertEqual(compathacks.branchset(repo), set(['default']))
        self.assertEqual(repo['tip'].manifest().keys(), ['flaf.txt'])

    def test_externals_single(self):
        repo = self._load_fixture_and_fetch('externals.svndump',
                                            layout='single')
        for rev in repo:
            assert '.hgsvnexternals' not in repo[rev].manifest()
        return # TODO enable test when externals in single are fixed
        expect = """[.]
 -r2 ^/externals/project2@2 deps/project2
[subdir]
 ^/externals/project1 deps/project1
[subdir2]
 ^/externals/project1 deps/project1
"""
        test = 2
        self.assertEqual(self.repo[test]['.hgsvnexternals'].data(), expect)

    def test_externals_single_whole_repo(self):
        # This is the test which demonstrates the brokenness of externals
        return # TODO enable test when externals in single are fixed
        repo = self._load_fixture_and_fetch('externals.svndump',
                                            layout='single',
                                            subdir='')
        for rev in repo:
            rc = repo[rev]
            if '.hgsvnexternals' in rc:
                extdata = rc['.hgsvnexternals'].data()
                assert '[.]' not in extdata
                print extdata
        expect = '' # Not honestly sure what this should be...
        test = 4
        self.assertEqual(self.repo[test]['.hgsvnexternals'].data(), expect)
