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

class TestSingleDirPush(test_util.TestBase):
    stupid_mode_tests = True
    obsolete_mode_tests = True

    def test_push_single_dir(self):
        # Tests simple pushing from default branch to a single dir repo
        repo, repo_path = self.load_and_fetch('branch_from_tag.svndump',
                                              layout='single',
                                              subdir='')
        def file_callback(repo, memctx, path):
            if path == 'adding_file':
                return compathacks.makememfilectx(repo,
                                                  path=path,
                                                  data='foo',
                                                  islink=False,
                                                  isexec=False,
                                                  copied=False)
            elif path == 'adding_binary':
                return compathacks.makememfilectx(repo,
                                                  path=path,
                                                  data='\0binary',
                                                  islink=False,
                                                  isexec=False,
                                                  copied=False)
            raise IOError(errno.EINVAL, 'Invalid operation: ' + path)
        ctx = context.memctx(repo,
                             (repo['tip'].node(), node.nullid),
                             'automated test',
                             ['adding_file', 'adding_binary'],
                             file_callback,
                             'an_author',
                             '2009-10-19 18:49:30 -0500',
                             {'branch': 'default', })
        repo.commitctx(ctx)
        hg.update(repo, repo['tip'].node())
        self.pushrevisions()
        self.assertTrue('adding_file' in test_util.svnls(repo_path, ''))
        self.assertEqual('application/octet-stream',
                         test_util.svnpropget(repo_path, 'adding_binary',
                                              'svn:mime-type'))
        # Now add another commit and test mime-type being reset
        changes = [('adding_binary', 'adding_binary', 'no longer binary')]
        self.commitchanges(changes)
        self.pushrevisions()
        self.assertEqual('', test_util.svnpropget(repo_path, 'adding_binary',
                                                  'svn:mime-type'))

    def test_push_single_dir_at_subdir(self):
        repo = self._load_fixture_and_fetch('branch_from_tag.svndump',
                                            layout='single',
                                            subdir='trunk')
        def filectxfn(repo, memctx, path):
            return compathacks.makememfilectx(repo,
                                              path=path,
                                              data='contents of %s' % path,
                                              islink=False,
                                              isexec=False,
                                              copied=False)
        ctx = context.memctx(repo,
                             (repo['tip'].node(), node.nullid),
                             'automated test',
                             ['bogus'],
                             filectxfn,
                             'an_author',
                             '2009-10-19 18:49:30 -0500',
                             {'branch': 'localhacking', })
        n = repo.commitctx(ctx)
        self.assertEqual(self.repo['tip']['bogus'].data(),
                         'contents of bogus')
        before = repo['tip'].hex()
        hg.update(repo, self.repo['tip'].hex())
        self.pushrevisions()
        self.assertNotEqual(before, self.repo['tip'].hex())
        self.assertEqual(self.repo['tip']['bogus'].data(),
                         'contents of bogus')

    def test_push_single_dir_one_incoming_and_two_outgoing(self):
        # Tests simple pushing from default branch to a single dir repo
        # Pushes two outgoing over one incoming svn rev
        # (used to cause an "unknown revision")
        # This can happen if someone committed to svn since our last pull (race).
        repo, repo_path = self.load_and_fetch('branch_from_tag.svndump',
                                              layout='single',
                                              subdir='trunk')
        self.add_svn_rev(repo_path, {'trunk/alpha': 'Changed'})
        def file_callback(repo, memctx, path):
            return compathacks.makememfilectx(repo,
                                              path=path,
                                              data='data of %s' % path,
                                              islink=False,
                                              isexec=False,
                                              copied=False)
        for fn in ['one', 'two']:
            ctx = context.memctx(repo,
                                 (repo['tip'].node(), node.nullid),
                                 'automated test',
                                 [fn],
                                 file_callback,
                                 'an_author',
                                 '2009-10-19 18:49:30 -0500',
                                 {'branch': 'default', })
            repo.commitctx(ctx)
        hg.update(repo, repo['tip'].node())
        self.pushrevisions(expected_extra_back=1)
        self.assertTrue('trunk/one' in test_util.svnls(repo_path, ''))
        self.assertTrue('trunk/two' in test_util.svnls(repo_path, ''))

    def test_push_single_dir_branch(self):
        # Tests local branches pushing to a single dir repo. Creates a fork at
        # tip. The default branch adds a file called default, while branch foo
        # adds a file called foo, then tries to push the foo branch and default
        # branch in that order.
        repo, repo_path = self.load_and_fetch('branch_from_tag.svndump',
                                              layout='single',
                                              subdir='')
        def file_callback(data):
            def cb(repo, memctx, path):
                if path == data:
                    return compathacks.makememfilectx(repo,
                                                      path=path,
                                                      data=data,
                                                      islink=False,
                                                      isexec=False,
                                                      copied=False)
                raise IOError(errno.EINVAL, 'Invalid operation: ' + path)
            return cb

        def commit_to_branch(name, parent):
            repo.commitctx(context.memctx(repo,
                                          (parent, node.nullid),
                                          'automated test (%s)' % name,
                                          [name],
                                          file_callback(name),
                                          'an_author',
                                          '2009-10-19 18:49:30 -0500',
                                          {'branch': name, }))

        parent = repo['tip'].node()
        commit_to_branch('default', parent)
        commit_to_branch('foo', parent)
        hg.update(repo, repo['foo'].node())
        self.pushrevisions()
        repo = self.repo # repo is outdated after the rebase happens, refresh
        self.assertTrue('foo' in test_util.svnls(repo_path, ''))
        self.assertEqual(compathacks.branchset(repo), set(['default']))
        # Have to cross to another branch head, so hg.update doesn't work
        commands.update(self.ui(),
                        self.repo,
                        self.repo.branchheads('default')[1],
                        clean=True)
        self.pushrevisions()
        self.assertTrue('default' in test_util.svnls(repo_path, ''))
        self.assertEquals(len(self.repo.branchheads('default')), 1)

    @test_util.requiresoption('branch')
    def test_push_single_dir_renamed_branch(self):
        # Tests pulling and pushing with a renamed branch
        # Based on test_push_single_dir
        repo_path = self.load_svndump('branch_from_tag.svndump')
        cmd = ['clone', '--quiet', '--layout=single', '--branch=flaf']
        if self.stupid:
            cmd.append('--stupid')
        cmd += [test_util.fileurl(repo_path), self.wc_path]
        test_util.dispatch(cmd)

        def file_callback(repo, memctx, path):
            if path == 'adding_file':
                return compathacks.makememfilectx(repo,
                                                  path=path,
                                                  data='foo',
                                                  islink=False,
                                                  isexec=False,
                                                  copied=False)
            raise IOError(errno.EINVAL, 'Invalid operation: ' + path)
        ctx = context.memctx(self.repo,
                             (self.repo['tip'].node(), node.nullid),
                             'automated test',
                             ['adding_file'],
                             file_callback,
                             'an_author',
                             '2009-10-19 18:49:30 -0500',
                             {'branch': 'default', })
        self.repo.commitctx(ctx)
        hg.update(self.repo, self.repo['tip'].node())
        self.pushrevisions()
        self.assertTrue('adding_file' in test_util.svnls(repo_path, ''))

        self.assertEquals(set(['flaf']),
                          set(self.repo[i].branch() for i in self.repo))
