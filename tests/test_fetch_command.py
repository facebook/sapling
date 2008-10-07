import os
import shutil
import tempfile
import unittest

from mercurial import hg
from mercurial import ui
from mercurial import node

import fetch_command
import util

class TestBasicRepoLayout(unittest.TestCase):
    def setUp(self):
        self.oldwd = os.getcwd()
        self.tmpdir = tempfile.mkdtemp('svnwrap_test')
        self.repo_path = '%s/testrepo' % self.tmpdir
        self.wc_path = '%s/testrepo_wc' % self.tmpdir

    def tearDown(self):
        shutil.rmtree(self.tmpdir)
        os.chdir(self.oldwd)

    def test_fresh_fetch_single_rev(self):
        util.load_svndump_fixture(self.repo_path, 'single_rev.svndump')
        fetch_command.fetch_revisions(ui.ui(), 
                                      svn_url='file://%s' % self.repo_path, 
                                      hg_repo_path=self.wc_path)
        repo = hg.repository(ui.ui(), self.wc_path)
        self.assertEqual(node.hex(repo['tip'].node()), 
                         'a47d0ce778660a91c31bf2c21c448e9ee296ac90')
        self.assertEqual(repo['tip'], repo[0])

    def test_fresh_fetch_two_revs(self):
        util.load_svndump_fixture(self.repo_path, 'two_revs.svndump')
        fetch_command.fetch_revisions(ui.ui(), 
                                      svn_url='file://%s' % self.repo_path, 
                                      hg_repo_path=self.wc_path)
        repo = hg.repository(ui.ui(), self.wc_path)
        # TODO there must be a better way than repo[0] for this check
        self.assertEqual(node.hex(repo[0].node()),
                         'a47d0ce778660a91c31bf2c21c448e9ee296ac90')
        self.assertEqual(node.hex(repo['tip'].node()),
                         'bf3767835b3b32ecc775a298c2fa27134dd91c11')
        self.assertEqual(repo['tip'], repo[1])
