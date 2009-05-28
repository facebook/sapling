import atexit
import os
import random
import shutil
import socket
import subprocess
import unittest

from mercurial import context
from mercurial import commands
from mercurial import hg
from mercurial import node
from mercurial import ui
from mercurial import revlog
from mercurial import util as hgutil

import test_util
import time


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

    def test_push_over_svnserve(self, commit=True):
        test_util.load_svndump_fixture(self.repo_path, 'simple_branch.svndump')
        open(os.path.join(self.repo_path, 'conf', 'svnserve.conf'),
             'w').write('[general]\nanon-access=write\n[sasl]\n')
        self.port = random.randint(socket.IPPORT_USERRESERVED, 65535)
        self.host = 'localhost'
        args = ['svnserve', '--daemon', '--foreground',
                '--listen-port=%d' % self.port,
                '--listen-host=%s' % self.host,
                '--root=%s' % self.repo_path]

        svnserve = subprocess.Popen(args, stdout=subprocess.PIPE,
                                    stderr=subprocess.STDOUT)
        self.svnserve_pid = svnserve.pid
        try:
            time.sleep(2)
            import shutil
            shutil.rmtree(self.wc_path)
            commands.clone(ui.ui(), 'svn://%s:%d/' % (self.host, self.port),
                           self.wc_path, noupdate=True)

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
                                 parents=(repo['default'].node(), node.nullid),
                                 text='automated test',
                                 files=['adding_file'],
                                 filectxfn=file_callback,
                                 user='an_author',
                                 date='2008-10-07 20:59:48 -0500',
                                 extra={'branch': 'default',})
            new_hash = repo.commitctx(ctx)
            if not commit:
                return # some tests use this test as an extended setup.
            hg.update(repo, repo['tip'].node())
            oldauthor = repo['tip'].user()
            commands.push(repo.ui, repo)
            tip = self.repo['tip']
            self.assertNotEqual(oldauthor, tip.user())
            self.assertNotEqual(tip.node(), old_tip)
            self.assertEqual(tip.parents()[0].node(), expected_parent)
            self.assertEqual(tip['adding_file'].data(), 'foo')
            self.assertEqual(tip.branch(), 'default')
            # unintended behaviour:
            self.assertNotEqual('an_author', tip.user())
            self.assertEqual('None', tip.user().rsplit('@', 1)[0])
        finally:
            os.kill(self.svnserve_pid, 9)

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
        commands.push(repo.ui, repo)
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

    def test_push_to_branch(self, push=True):
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
        hg.update(repo, repo['tip'].node())
        if push:
            self.pushrevisions()
            tip = self.repo['tip']
            self.assertNotEqual(tip.node(), new_hash)
            self.assertEqual(tip['adding_file'].data(), 'foo')
            self.assertEqual(tip.branch(), 'the_branch')

    def push_to_non_tip(self):
        self.test_push_to_branch(push=False)
        wc2path = self.wc_path + '_clone'
        u = self.repo.ui
        hg.clone(self.repo.ui, self.wc_path, wc2path, update=False)
        self.pushrevisions()
        oldf = open(os.path.join(self.wc_path, '.hg', 'hgrc'))
        hgrc = oldf.read()
        oldf.close()
        shutil.rmtree(self.wc_path)
        hg.clone(u, wc2path, self.wc_path, update=False)
        oldf = open(os.path.join(self.wc_path, '.hg', 'hgrc'), 'w')
        oldf.write(hgrc)
        oldf.close()

        # do a commit here
        self.commitchanges([('foobaz', 'foobaz', 'This file is added on default.', ),
                            ],
                           parent='default',
                           message='commit to default')
        from hgsubversion import svncommands
        svncommands.rebuildmeta(u,
                                self.repo,
                                os.path.dirname(self.repo.path),
                                args=[test_util.fileurl(self.repo_path)])


        hg.update(self.repo, self.repo['tip'].node())
        oldnode = self.repo['tip'].hex()
        self.pushrevisions(expected_extra_back=1)
        self.assertNotEqual(oldnode, self.repo['tip'].hex(), 'Revision was not pushed.')

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
    test_classes = [PushTests, ]
    tests = []
    # This is the quickest hack I could come up with to load all the tests from
    # both classes. Would love a patch that simplifies this without adding
    # dependencies.
    for tc in test_classes:
        for attr in dir(tc):
            if attr.startswith('test_'):
                tests.append(tc(attr))
    return unittest.TestSuite(tests)
