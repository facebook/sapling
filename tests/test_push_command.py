import os
import shutil
import socket
import tempfile
import unittest

from mercurial import context
from mercurial import commands
from mercurial import hg
from mercurial import node
from mercurial import ui
from mercurial import revlog

import fetch_command
import push_cmd
import test_util
import time


class PushOverSvnserveTests(unittest.TestCase):
    def setUp(self):
        self.oldwd = os.getcwd()
        self.tmpdir = tempfile.mkdtemp('svnwrap_test')
        self.repo_path = '%s/testrepo' % self.tmpdir
        self.wc_path = '%s/testrepo_wc' % self.tmpdir
        test_util.load_svndump_fixture(self.repo_path, 'simple_branch.svndump')
        open(os.path.join(self.repo_path, 'conf', 'svnserve.conf'),
             'w').write('[general]\nanon-access=write\n[sasl]\n')
        # Paranoia: we try and connect to localhost on 3689 before we start
        # svnserve. If it is running, we force the test to fail early.
        user_has_own_svnserve = False
        try:
            s = socket.socket()
            s.settimeout(0.3)
            s.connect(('localhost', 3690))
            s.close()
            user_has_own_svnserve = True
        except:
            pass
        if user_has_own_svnserve:
            assert False, ('You appear to be running your own svnserve!'
                           ' You can probably ignore this test failure.')
        args = ['svnserve', '-d', '--foreground', '-r', self.repo_path]
        self.svnserve_pid = os.spawnvp(os.P_NOWAIT, 'svnserve', args)
        time.sleep(2)
        fetch_command.fetch_revisions(ui.ui(),
                                      svn_url='svn://localhost/',
                                      hg_repo_path=self.wc_path)

    def tearDown(self):
        shutil.rmtree(self.tmpdir)
        os.chdir(self.oldwd)
        os.system('kill -9 %d' % self.svnserve_pid)

    # define this as a property so that it reloads anytime we need it
    @property
    def repo(self):
        return hg.repository(ui.ui(), self.wc_path)

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
                                              svn_url='svn://localhost/')
        tip = self.repo['tip']
        self.assertNotEqual(tip.node(), old_tip)
        self.assertEqual(tip.parents()[0].node(), expected_parent)
        self.assertEqual(tip['adding_file'].data(), 'foo')
        self.assertEqual(tip.branch(), 'default')


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
        #commands.update(ui.ui(), self.repo, node='tip')
        hg.update(repo, repo['tip'].node())
        push_cmd.push_revisions_to_subversion(ui.ui(), repo=self.repo,
                                              hg_repo_path=self.wc_path,
                                              svn_url='file://'+self.repo_path)
        tip = self.repo['tip']
        self.assertNotEqual(tip.node(), new_hash)
        self.assertEqual(tip['adding_file'].data(), 'foo')
        self.assertEqual(tip.branch(), 'the_branch')

    def test_delete_file(self):
        repo = self.repo
        def file_callback(repo, memctx, path):
            raise IOError()
        old_files = set(repo['default'].manifest().keys())
        ctx = context.memctx(repo,
                             (repo['default'].node(), node.nullid),
                             'automated test',
                             ['alpha'],
                             file_callback,
                             'an author',
                             '2008-10-29 21:26:00 -0500',
                             {'branch': 'default', })
        new_hash = repo.commitctx(ctx)
        hg.update(repo, repo['tip'].node())
        push_cmd.push_revisions_to_subversion(ui.ui(), repo=self.repo,
                                              hg_repo_path=self.wc_path,
                                              svn_url='file://' + self.repo_path)
        tip = self.repo['tip']
        self.assertEqual(old_files,
                         set(tip.manifest().keys() + ['alpha']))
        self.assert_('alpha' not in tip.manifest())

    def test_push_executable_file(self):
        self.test_push_to_default(commit=True)
        repo = self.repo
        def file_callback(repo, memctx, path):
            if path == 'gamma':
                return context.memfilectx(path=path,
                                          data='foo',
                                          islink=False,
                                          isexec=True,
                                          copied=False)
            raise IOError()
        ctx = context.memctx(repo,
                             (repo['tip'].node(), node.nullid),
                             'message',
                             ['gamma', ],
                             file_callback,
                             'author',
                             '2008-10-29 21:26:00 -0500',
                             {'branch': 'default', })
        new_hash = repo.commitctx(ctx)
        hg.update(repo, repo['tip'].node())
        push_cmd.push_revisions_to_subversion(ui.ui(), repo=self.repo,
                                              hg_repo_path=self.wc_path,
                                              svn_url='file://' + self.repo_path)
        tip = self.repo['tip']
        self.assertNotEqual(tip.node(), new_hash)
        self.assert_('@' in tip.user())
        self.assertEqual(tip['gamma'].flags(), 'x')
        self.assertEqual(tip['gamma'].data(), 'foo')
        self.assertEqual([x for x in tip.manifest().keys() if 'x' not in
                          tip[x].flags()], ['alpha', 'beta', 'adding_file', ])

    def test_push_symlink_file(self):
        self.test_push_to_default(commit=True)
        repo = self.repo
        def file_callback(repo, memctx, path):
            if path == 'gamma':
                return context.memfilectx(path=path,
                                          data='foo',
                                          islink=True,
                                          isexec=False,
                                          copied=False)
            raise IOError()
        ctx = context.memctx(repo,
                             (repo['tip'].node(), node.nullid),
                             'message',
                             ['gamma', ],
                             file_callback,
                             'author',
                             '2008-10-29 21:26:00 -0500',
                             {'branch': 'default', })
        new_hash = repo.commitctx(ctx)
        hg.update(repo, repo['tip'].node())
        push_cmd.push_revisions_to_subversion(ui.ui(), repo=self.repo,
                                              hg_repo_path=self.wc_path,
                                              svn_url='file://' + self.repo_path)
        tip = self.repo['tip']
        self.assertNotEqual(tip.node(), new_hash)
        self.assertEqual(tip['gamma'].flags(), 'l')
        self.assertEqual(tip['gamma'].data(), 'foo')
        self.assertEqual([x for x in tip.manifest().keys() if 'l' not in
                          tip[x].flags()], ['alpha', 'beta', 'adding_file', ])

    def test_push_with_new_dir(self):
        self.test_push_to_default(commit=True)
        repo = self.repo
        def file_callback(repo, memctx, path):
            if path == 'newdir/gamma':
                return context.memfilectx(path=path,
                                          data='foo',
                                          islink=False,
                                          isexec=False,
                                          copied=False)
            raise IOError()
        ctx = context.memctx(repo,
                             (repo['tip'].node(), node.nullid),
                             'message',
                             ['newdir/gamma', ],
                             file_callback,
                             'author',
                             '2008-10-29 21:26:00 -0500',
                             {'branch': 'default', })
        new_hash = repo.commitctx(ctx)
        hg.update(repo, repo['tip'].node())
        push_cmd.push_revisions_to_subversion(ui.ui(), repo=self.repo,
                                              hg_repo_path=self.wc_path,
                                              svn_url='file://' + self.repo_path)
        tip = self.repo['tip']
        self.assertNotEqual(tip.node(), new_hash)
        self.assertEqual(tip['newdir/gamma'].data(), 'foo')

    def test_push_existing_file_newly_execute(self, execute=True,
                                              link=False, expected_flags='x'):
        self.test_push_to_default()
        repo = self.repo
        def file_callback(repo, memctx, path):
            return context.memfilectx(path=path,
                                      data='foo',
                                      islink=link,
                                      isexec=execute,
                                      copied=False)
        ctx = context.memctx(repo,
                             (repo['default'].node(), node.nullid),
                             'message',
                             ['alpha', ],
                             file_callback,
                             'author',
                             '2008-1-1 00:00:00 -0500',
                             {'branch': 'default', })
        new_hash = repo.commitctx(ctx)
        hg.update(repo, repo['tip'].node())
        push_cmd.push_revisions_to_subversion(ui.ui(), repo=self.repo,
                                              hg_repo_path=self.wc_path,
                                              svn_url='file://' + self.repo_path)
        tip = self.repo['tip']
        self.assertNotEqual(tip.node(), new_hash)
        self.assertEqual(tip['alpha'].data(), 'foo')
        self.assertEqual(tip.parents()[0]['alpha'].flags(), '')
        self.assertEqual(tip['alpha'].flags(), expected_flags)
        # while we're here, double check pushing an already-executable file
        # works
        repo = self.repo
        def file_callback(repo, memctx, path):
            return context.memfilectx(path=path,
                                      data='bar',
                                      islink=link,
                                      isexec=execute,
                                      copied=False)
        ctx = context.memctx(repo,
                             (repo['default'].node(), node.nullid),
                             'message',
                             ['alpha', ],
                             file_callback,
                             'author',
                             '2008-1-1 00:00:00 -0500',
                             {'branch': 'default', })
        new_hash = repo.commitctx(ctx)
        hg.update(repo, repo['tip'].node())
        push_cmd.push_revisions_to_subversion(ui.ui(), repo=self.repo,
                                              hg_repo_path=self.wc_path,
                                              svn_url='file://' + self.repo_path)
        tip = self.repo['tip']
        self.assertNotEqual(tip.node(), new_hash)
        self.assertEqual(tip['alpha'].data(), 'bar')
        self.assertEqual(tip.parents()[0]['alpha'].flags(), expected_flags)
        self.assertEqual(tip['alpha'].flags(), expected_flags)


def suite():
    test_classes = [PushTests, PushOverSvnserveTests]
    tests = []
    # This is the quickest hack I could come up with to load all the tests from
    # both classes. Would love a patch that simplifies this without adding
    # dependencies.
    for tc in test_classes:
        for attr in dir(tc):
            if attr.startswith('test_'):
                tests.append(tc(attr))
    return unittest.TestSuite(tests)
