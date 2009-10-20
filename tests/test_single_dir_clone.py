import shutil

from mercurial import commands
from mercurial import context
from mercurial import hg
from mercurial import node
from mercurial import ui

import test_util


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
        repo = self._load_fixture_and_fetch('branch_from_tag.svndump',
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
            raise IOError()
        ctx = context.memctx(repo,
                             (repo['tip'].node(), node.nullid),
                             'automated test',
                             ['adding_file'],
                             file_callback,
                             'an_author',
                             '2009-10-19 18:49:30 -0500',
                             {'branch': 'default',})
        repo.commitctx(ctx)
        hg.update(repo, repo['tip'].node())
        self.pushrevisions()
        self.assertTrue('adding_file' in self.svnls(''))

    def test_push_single_dir_branch(self):
        # Tests local branches pushing to a single dir repo. Creates a fork at
        # tip. The default branch adds a file called default, while branch foo
        # adds a file called foo, then tries to push the foo branch and default
        # branch in that order.
        repo = self._load_fixture_and_fetch('branch_from_tag.svndump',
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
                raise IOError()
            return cb

        def commit_to_branch(name, parent):
            repo.commitctx(context.memctx(repo,
                                          (parent, node.nullid),
                                          'automated test (%s)' % name,
                                          [name],
                                          file_callback(name),
                                          'an_author',
                                          '2009-10-19 18:49:30 -0500',
                                          {'branch': name,}))

        parent = repo['tip'].node()
        commit_to_branch('default', parent)
        commit_to_branch('foo', parent)
        hg.update(repo, repo['foo'].node())
        self.pushrevisions()
        repo = self.repo # repo is outdated after the rebase happens, refresh
        self.assertTrue('foo' in self.svnls(''))
        self.assertEqual(repo.branchtags().keys(), ['default'])
        # Have to cross to another branch head, so hg.update doesn't work
        commands.update(ui.ui(),
                        self.repo,
                        self.repo.branchheads('default')[1],
                        clean=True)
        self.pushrevisions()
        self.assertTrue('default' in self.svnls(''))
        self.assertEquals(len(self.repo.branchheads('default')), 1)

