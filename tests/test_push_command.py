import os
import socket
import subprocess
import unittest

from mercurial import context
from mercurial import hg
from mercurial import node
from mercurial import ui
from mercurial import revlog
from mercurial import util as hgutil

import wrappers
import test_util
import time


class PushOverSvnserveTests(test_util.TestBase):
    def setUp(self):
        test_util.TestBase.setUp(self)
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
        self.svnserve_pid = subprocess.Popen(args).pid
        time.sleep(2)
        wrappers.clone(None, ui.ui(), source='svn://localhost/',
                       dest=self.wc_path, noupdate=True)

    def tearDown(self):
        os.system('kill -9 %d' % self.svnserve_pid)
        test_util.TestBase.tearDown(self)

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
        oldauthor = repo['tip'].user()
        wrappers.push(None, ui.ui(), repo=self.repo)
        tip = self.repo['tip']
        self.assertNotEqual(oldauthor, tip.user())
        self.assertNotEqual(tip.node(), old_tip)
        self.assertEqual(tip.parents()[0].node(), expected_parent)
        self.assertEqual(tip['adding_file'].data(), 'foo')
        self.assertEqual(tip.branch(), 'default')


class PushTests(test_util.TestBase):
    def setUp(self):
        test_util.TestBase.setUp(self)
        test_util.load_fixture_and_fetch('simple_branch.svndump',
                                         self.repo_path,
                                         self.wc_path)

    def test_cant_push_empty_ctx(self):
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
                             (repo['default'].node(), node.nullid),
                             'automated test',
                             [],
                             file_callback,
                             'an_author',
                             '2008-10-07 20:59:48 -0500',
                             {'branch': 'default',})
        new_hash = repo.commitctx(ctx)
        hg.update(repo, repo['tip'].node())
        old_tip = repo['tip'].node()
        self.pushrevisions()
        tip = self.repo['tip']
        self.assertEqual(tip.node(), old_tip)


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
        self.pushrevisions()
        tip = self.repo['tip']
        self.assertNotEqual(tip.node(), old_tip)
        self.assertEqual(node.hex(tip.parents()[0].node()),
                         node.hex(expected_parent))
        self.assertEqual(tip['adding_file'].data(), 'foo')
        self.assertEqual(tip.branch(), 'default')

    def test_push_two_revs_different_local_branch(self):
        def filectxfn(repo, memctx, path):
            return context.memfilectx(path=path,
                                      data=path,
                                      islink=False,
                                      isexec=False,
                                      copied=False)
        oldtiphash = self.repo['default'].node()
        ctx = context.memctx(self.repo,
                             (self.repo[0].node(), revlog.nullid, ),
                             'automated test',
                             ['gamma', ],
                             filectxfn,
                             'testy',
                             '2008-12-21 16:32:00 -0500',
                             {'branch': 'localbranch', })
        newhash = self.repo.commitctx(ctx)
        ctx = context.memctx(self.repo,
                             (newhash, revlog.nullid),
                             'automated test2',
                             ['delta', ],
                             filectxfn,
                             'testy',
                             '2008-12-21 16:32:00 -0500',
                             {'branch': 'localbranch', })
        newhash = self.repo.commitctx(ctx)
        repo = self.repo
        hg.update(repo, newhash)
        wrappers.push(None, ui.ui(), repo=repo)
        self.assertEqual(self.repo['tip'].parents()[0].parents()[0].node(), oldtiphash)
        self.assertEqual(self.repo['tip'].files(), ['delta', ])
        self.assertEqual(self.repo['tip'].manifest().keys(),
                         ['alpha', 'beta', 'gamma', 'delta'])

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
        self.pushrevisions()
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
        self.pushrevisions()
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
        self.pushrevisions()
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
        hg.clean(repo, repo['tip'].node())
        self.pushrevisions()
        tip = self.repo['tip']
        self.assertNotEqual(tip.node(), new_hash)
        self.assert_('@' in self.repo['tip'].user())
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
        self.pushrevisions()
        tip = self.repo['tip']
        self.assertNotEqual(tip.node(), new_hash)
        self.assertEqual(tip['gamma'].flags(), 'l')
        self.assertEqual(tip['gamma'].data(), 'foo')
        self.assertEqual([x for x in tip.manifest().keys() if 'l' not in
                          tip[x].flags()], ['alpha', 'beta', 'adding_file', ])

    def test_push_existing_file_newly_symlink(self):
        self.test_push_existing_file_newly_execute(execute=False,
                                                   link=True,
                                                   expected_flags='l')

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
        self.pushrevisions()
        tip = self.repo['tip']
        self.assertNotEqual(tip.node(), new_hash)
        self.assertEqual(tip['alpha'].data(), 'foo')
        self.assertEqual(tip.parents()[0]['alpha'].flags(), '')
        self.assertEqual(tip['alpha'].flags(), expected_flags)
        # while we're here, double check pushing an already-executable file
        # works
        repo = self.repo
        def file_callback2(repo, memctx, path):
            return context.memfilectx(path=path,
                                      data='bar',
                                      islink=link,
                                      isexec=execute,
                                      copied=False)
        ctx = context.memctx(repo,
                             (repo['default'].node(), node.nullid),
                             'message',
                             ['alpha', ],
                             file_callback2,
                             'author',
                             '2008-1-1 00:00:00 -0500',
                             {'branch': 'default', })
        new_hash = repo.commitctx(ctx)
        hg.update(repo, repo['tip'].node())
        self.pushrevisions()
        tip = self.repo['tip']
        self.assertNotEqual(tip.node(), new_hash)
        self.assertEqual(tip['alpha'].data(), 'bar')
        self.assertEqual(tip.parents()[0]['alpha'].flags(), expected_flags)
        self.assertEqual(tip['alpha'].flags(), expected_flags)
        # now test removing the property entirely
        repo = self.repo
        def file_callback3(repo, memctx, path):
            return context.memfilectx(path=path,
                                      data='bar',
                                      islink=False,
                                      isexec=False,
                                      copied=False)
        ctx = context.memctx(repo,
                             (repo['default'].node(), node.nullid),
                             'message',
                             ['alpha', ],
                             file_callback3,
                             'author',
                             '2008-01-01 00:00:00 -0500',
                             {'branch': 'default', })
        new_hash = repo.commitctx(ctx)
        hg.update(repo, repo['tip'].node())
        self.pushrevisions()
        tip = self.repo['tip']
        self.assertNotEqual(tip.node(), new_hash)
        self.assertEqual(tip['alpha'].data(), 'bar')
        self.assertEqual(tip.parents()[0]['alpha'].flags(), expected_flags)
        self.assertEqual(tip['alpha'].flags(), '')

    def test_push_outdated_base_text(self):
        self.test_push_two_revs()
        changes = [('adding_file', 'adding_file', 'different_content', ),
                   ]
        self.commitchanges(changes, parent='tip')
        self.pushrevisions()
        changes = [('adding_file', 'adding_file',
                    'even_more different_content', ),
                   ]
        self.commitchanges(changes, parent=3)
        try:
            self.pushrevisions()
            assert False, 'This should have aborted!'
        except hgutil.Abort, e:
            self.assertEqual(e.args[0],
                             'Base text was out of date, maybe rebase?')


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
