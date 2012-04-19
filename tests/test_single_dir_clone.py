import test_util

import errno
import shutil
import unittest

from mercurial import commands
from mercurial import context
from mercurial import hg
from mercurial import node
from mercurial import ui

class TestSingleDir(test_util.TestBase):
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

    def test_push_single_dir(self):
        # Tests simple pushing from default branch to a single dir repo
        repo, repo_path = self.load_and_fetch('branch_from_tag.svndump',
                                              stupid=False,
                                              layout='single',
                                              subdir='')
        def file_callback(repo, memctx, path):
            if path == 'adding_file':
                return context.memfilectx(path=path,
                                          data='foo',
                                          islink=False,
                                          isexec=False,
                                          copied=False)
            elif path == 'adding_binary':
                return context.memfilectx(path=path,
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
                                            stupid=False,
                                            layout='single',
                                            subdir='trunk')
        def filectxfn(repo, memctx, path):
            return context.memfilectx(path=path,
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
                                              stupid=False,
                                              layout='single',
                                              subdir='trunk')
        self.add_svn_rev(repo_path, {'trunk/alpha': 'Changed'})
        def file_callback(repo, memctx, path):
            return context.memfilectx(path=path,
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
                                              stupid=False,
                                              layout='single',
                                              subdir='')
        def file_callback(data):
            def cb(repo, memctx, path):
                if path == data:
                    return context.memfilectx(path=path,
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
        self.assertEqual(repo.branchtags().keys(), ['default'])
        # Have to cross to another branch head, so hg.update doesn't work
        commands.update(ui.ui(),
                        self.repo,
                        self.repo.branchheads('default')[1],
                        clean=True)
        self.pushrevisions()
        self.assertTrue('default' in test_util.svnls(repo_path, ''))
        self.assertEquals(len(self.repo.branchheads('default')), 1)

    @test_util.requiresoption('branch')
    def test_push_single_dir_renamed_branch(self, stupid=False):
        # Tests pulling and pushing with a renamed branch
        # Based on test_push_single_dir
        repo_path = self.load_svndump('branch_from_tag.svndump')
        cmd = ['clone', '--layout=single', '--branch=flaf']
        if stupid:
            cmd.append('--stupid')
        cmd += [test_util.fileurl(repo_path), self.wc_path]
        test_util.dispatch(cmd)

        def file_callback(repo, memctx, path):
            if path == 'adding_file':
                return context.memfilectx(path=path,
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

    @test_util.requiresoption('branch')
    def test_push_single_dir_renamed_branch_stupid(self):
        self.test_push_single_dir_renamed_branch(True)

def suite():
    all_tests = [unittest.TestLoader().loadTestsFromTestCase(TestSingleDir)]
    return unittest.TestSuite(all_tests)
