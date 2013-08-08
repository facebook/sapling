import test_util

import errno
import shutil
import unittest

from mercurial import commands
from mercurial import context
from mercurial import hg
from mercurial import node
from mercurial import ui

class TestSingleDirClone(test_util.TestBase):
    def test_clone_single_dir_simple(self):
        repo = self._load_fixture_and_fetch('branch_from_tag.svndump',
                                            stupid=False,
                                            layout='single',
                                            subdir='')
        self.assertEqual(repo.branchtags().keys(), ['default'])
        self.assertEqual(repo['tip'].manifest().keys(),
                         ['trunk/beta',
                          'tags/copied_tag/alpha',
                          'trunk/alpha',
                          'tags/copied_tag/beta',
                          'branches/branch_from_tag/alpha',
                          'tags/tag_r3/alpha',
                          'tags/tag_r3/beta',
                          'branches/branch_from_tag/beta'])

    def test_auto_detect_single(self):
        repo = self._load_fixture_and_fetch('branch_from_tag.svndump',
                                            stupid=False,
                                            layout='auto')
        self.assertEqual(repo.branchtags().keys(), ['default',
                                                    'branch_from_tag'])
        oldmanifest = test_util.filtermanifest(repo['default'].manifest().keys())
        # remove standard layout
        shutil.rmtree(self.wc_path)
        # try again with subdir to get single dir clone
        repo = self._load_fixture_and_fetch('branch_from_tag.svndump',
                                            stupid=False,
                                            layout='auto',
                                            subdir='trunk')
        self.assertEqual(repo.branchtags().keys(), ['default', ])
        self.assertEqual(repo['default'].manifest().keys(), oldmanifest)

    def test_clone_subdir_is_file_prefix(self, stupid=False):
        FIXTURE = 'subdir_is_file_prefix.svndump'
        repo = self._load_fixture_and_fetch(FIXTURE,
                                            stupid=stupid,
                                            layout='single',
                                            subdir=test_util.subdir[FIXTURE])
        self.assertEqual(repo.branchtags().keys(), ['default'])
        self.assertEqual(repo['tip'].manifest().keys(), ['flaf.txt'])

    def test_externals_single(self):
        repo = self._load_fixture_and_fetch('externals.svndump',
                                            stupid=False,
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
                                            stupid=False,
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
