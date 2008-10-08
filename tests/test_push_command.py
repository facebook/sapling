import os
import shutil
import tempfile
import unittest

from mercurial import context
from mercurial import hg
from mercurial import node
from mercurial import ui
from mercurial import revlog

import fetch_command
import push_cmd
import test_util

class PushTests(unittest.TestCase):
    def setUp(self):
        self.oldwd = os.getcwd()
        self.tmpdir = tempfile.mkdtemp('svnwrap_test')
        self.repo_path = '%s/testrepo' % self.tmpdir
        self.wc_path = '%s/testrepo_wc' % self.tmpdir
        test_util.load_svndump_fixture(self.repo_path, 'simple_branch.svndump')
        fetch_command.fetch_revisions(ui.ui(), 
                                      svn_url='file://%s' % self.repo_path, 
                                      hg_repo_path=self.wc_path)

    # define this as a property so that it reloads anytime we need it
    @property
    def repo(self):
        return hg.repository(ui.ui(), self.wc_path)

    def tearDown(self):
        shutil.rmtree(self.tmpdir)
        os.chdir(self.oldwd)

    def test_push_to_default(self, commit=True):
        repo = self.repo
        old_tip = repo['tip'].node()
        expected_parent = repo['default'].node()
        def file_callback(repo, memctx, path):
            if path == 'adding_file':
                return context.memfilectx(path=path,
                                          data='foo',
                                          islink=False,
                                          isexec=False,
                                          copied=False)
            raise IOError()
        ctx = context.memctx(repo,
                             (repo['default'].node(), node.nullid),
                             'automated test',
                             ['adding_file'],
                             file_callback,
                             'an_author',
                             '2008-10-07 20:59:48 -0500',
                             {'branch': 'default',})
        new_hash = repo.commitctx(ctx)
        if not commit:
            return # some tests use this test as an extended setup.
        hg.update(repo, repo['tip'].node())
        push_cmd.push_revisions_to_subversion(ui.ui(), repo=self.repo,
                                              hg_repo_path=self.wc_path,
                                              svn_url='file://'+self.repo_path)
        tip = self.repo['tip']
        self.assertNotEqual(tip.node(), old_tip)
        self.assertEqual(tip.parents()[0].node(), expected_parent)
        self.assertEqual(tip['adding_file'].data(), 'foo')
        self.assertEqual(tip.branch(), 'default')

    def test_push_two_revs(self):
        # set up some work for us
        self.test_push_to_default(commit=False)
        repo = self.repo
        old_tip = repo['tip'].node()
        expected_parent = repo['tip'].parents()[0].node()
        def file_callback(repo, memctx, path):
            if path == 'adding_file2':
                return context.memfilectx(path=path,
                                          data='foo2',
                                          islink=False,
                                          isexec=False,
                                          copied=False)
            raise IOError()
        ctx = context.memctx(repo,
                             (repo['default'].node(), node.nullid),
                             'automated test',
                             ['adding_file2'],
                             file_callback,
                             'an_author',
                             '2008-10-07 20:59:48 -0500',
                             {'branch': 'default',})
        new_hash = repo.commitctx(ctx)
        hg.update(repo, repo['tip'].node())
        push_cmd.push_revisions_to_subversion(ui.ui(), repo=self.repo,
                                              hg_repo_path=self.wc_path,
                                              svn_url='file://'+self.repo_path)
        tip = self.repo['tip']
        self.assertNotEqual(tip.node(), old_tip)
        self.assertNotEqual(tip.parents()[0].node(), old_tip)
        self.assertEqual(tip.parents()[0].parents()[0].node(), expected_parent)
        self.assertEqual(tip['adding_file2'].data(), 'foo2')
        self.assertEqual(tip['adding_file'].data(), 'foo')
        self.assertEqual(tip.parents()[0]['adding_file'].data(), 'foo')
        try:
            self.assertEqual(tip.parents()[0]['adding_file2'].data(), 'foo')
            assert False, "this is impossible, adding_file2 should not be in this manifest."
        except revlog.LookupError, e:
            pass
        self.assertEqual(tip.branch(), 'default')

    def test_push_to_branch(self):
        repo = self.repo
        def file_callback(repo, memctx, path):
            if path == 'adding_file':
                return context.memfilectx(path=path,
                                          data='foo',
                                          islink=False,
                                          isexec=False,
                                          copied=False)
            raise IOError()
        ctx = context.memctx(repo,
                             (repo['the_branch'].node(), node.nullid),
                             'automated test',
                             ['adding_file'],
                             file_callback,
                             'an_author',
                             '2008-10-07 20:59:48 -0500',
                             {'branch': 'the_branch',})
        new_hash = repo.commitctx(ctx)
        push_cmd.push_revisions_to_subversion(ui.ui(), repo=self.repo,
                                              hg_repo_path=self.wc_path,
                                              svn_url='file://'+self.repo_path)
        tip = self.repo['tip']
        self.assertEqual(tip['adding_file'].data(), 'foo')
        self.assertEqual(tip.branch(), 'the_branch')

#
#    def test_delete_file(self):
#        assert False
#
#    def test_push_executable_file(self):
#        assert False
#
#    def test_push_symlink_file(self):
#        assert False

def suite():
    return unittest.TestLoader().loadTestsFromTestCase(PushTests)
