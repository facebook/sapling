import test_util

import os
import unittest

from hgext import rebase
from mercurial import hg
from mercurial import revlog
from mercurial import context
from mercurial import node
from mercurial import commands
from mercurial import util as hgutil

from hgsubversion import util
from hgsubversion import svncommands
from hgsubversion import wrappers

expected_info_output = '''URL: %(repourl)s/%(branch)s
Repository Root: %(repourl)s
Repository UUID: df2126f7-00ab-4d49-b42c-7e981dde0bcf
Revision: %(rev)s
Node Kind: directory
Last Changed Author: durin
Last Changed Rev: %(rev)s
Last Changed Date: %(date)s
'''

class UtilityTests(test_util.TestBase):
    @property
    def repourl(self):
        return util.normalize_url(test_util.fileurl(self.repo_path))

    def test_info_output(self):
        self._load_fixture_and_fetch('two_heads.svndump')
        hg.update(self.repo, 'the_branch')
        u = self.ui()
        u.pushbuffer()
        svncommands.info(u, self.repo)
        actual = u.popbuffer()
        expected = (expected_info_output %
                    {'date': '2008-10-08 01:39:05 +0000 (Wed, 08 Oct 2008)',
                     'repourl': self.repourl,
                     'branch': 'branches/the_branch',
                     'rev': 5,
                     })
        self.assertMultiLineEqual(actual, expected)
        hg.update(self.repo, 'default')
        u.pushbuffer()
        svncommands.info(u, self.repo)
        actual = u.popbuffer()
        expected = (expected_info_output %
                    {'date': '2008-10-08 01:39:29 +0000 (Wed, 08 Oct 2008)',
                     'repourl': self.repourl,
                     'branch': 'trunk',
                     'rev': 6,
                     })
        self.assertMultiLineEqual(actual, expected)
        hg.update(self.repo, 'default')
        u.pushbuffer()
        svncommands.info(u, self.repo, rev=3)
        actual = u.popbuffer()
        expected = (expected_info_output %
                    {'date': '2008-10-08 01:39:05 +0000 (Wed, 08 Oct 2008)',
                     'repourl': self.repourl,
                     'branch': 'branches/the_branch',
                     'rev': 5,
                     })
        self.assertMultiLineEqual(actual, expected)

    def test_info_single(self):
        self._load_fixture_and_fetch('two_heads.svndump', subdir='trunk')
        hg.update(self.repo, 'tip')
        u = self.ui()
        u.pushbuffer()
        svncommands.info(u, self.repo)
        actual = u.popbuffer()
        expected = (expected_info_output %
                    {'date': '2008-10-08 01:39:29 +0000 (Wed, 08 Oct 2008)',
                     'repourl': self.repourl,
                     'branch': 'trunk',
                     'rev': 6,
                     })
        self.assertMultiLineEqual(expected, actual)

    def test_info_missing_metadata(self):
        repo = self._load_fixture_and_fetch('two_heads.svndump')
        test_util.rmtree(repo.join('svn'))
        self.assertRaises(hgutil.Abort,
                          repo.svnmeta)
        self.assertRaises(hgutil.Abort,
                          svncommands.info, self.ui, self.repo)

    def test_parent_output(self):
        self._load_fixture_and_fetch('two_heads.svndump')
        u = self.ui()
        u.pushbuffer()
        parents = (self.repo['the_branch'].node(), revlog.nullid, )
        def filectxfn(repo, memctx, path):
            return context.memfilectx(path=path,
                                      data='added',
                                      islink=False,
                                      isexec=False,
                                      copied=False)
        ctx = context.memctx(self.repo,
                             parents,
                             'automated test',
                             ['added_bogus_file', 'other_added_file', ],
                             filectxfn,
                             'testy',
                             '2008-12-21 16:32:00 -0500',
                             {'branch': 'localbranch', })
        new = self.repo.commitctx(ctx)
        hg.update(self.repo, new)
        wrappers.parents(lambda x, y: None, u, self.repo, svn=True)
        actual = u.popbuffer()
        self.assertEqual(actual, '3:4e256962fc5d\n')

        hg.update(self.repo, 'default')

        # Make sure styles work
        u.pushbuffer()
        wrappers.parents(lambda x, y: None, u, self.repo, svn=True, style='compact')
        actual = u.popbuffer()
        self.assertEqual(actual, '4:1083037b18d8\n')

        # custom templates too
        u.pushbuffer()
        wrappers.parents(lambda x, y: None, u, self.repo, svn=True, template='{node}\n')
        actual = u.popbuffer()
        self.assertEqual(actual, '1083037b18d85cd84fa211c5adbaeff0fea2cd9f\n')

        u.pushbuffer()
        wrappers.parents(lambda x, y: None, u, self.repo, svn=True)
        actual = u.popbuffer()
        self.assertEqual(actual, '4:1083037b18d8\n')

    def test_outgoing_output(self):
        self._load_fixture_and_fetch('two_heads.svndump')
        u = self.ui()
        parents = (self.repo['the_branch'].node(), revlog.nullid, )
        def filectxfn(repo, memctx, path):
            return context.memfilectx(path=path,
                                      data='added',
                                      islink=False,
                                      isexec=False,
                                      copied=False)
        ctx = context.memctx(self.repo,
                             parents,
                             'automated test',
                             ['added_bogus_file', 'other_added_file', ],
                             filectxfn,
                             'testy',
                             '2008-12-21 16:32:00 -0500',
                             {'branch': 'localbranch', })
        new = self.repo.commitctx(ctx)
        hg.update(self.repo, new)
        u.pushbuffer()
        commands.outgoing(u, self.repo, self.repourl)
        actual = u.popbuffer()
        self.assertTrue(node.hex(self.repo['localbranch'].node())[:8] in actual)
        self.assertEqual(actual.strip(), '5:6de15430fa20')
        hg.update(self.repo, 'default')
        u.pushbuffer()
        commands.outgoing(u, self.repo, self.repourl)
        actual = u.popbuffer()
        self.assertEqual(actual, '')

    def test_rebase(self):
        self._load_fixture_and_fetch('two_revs.svndump')
        parents = (self.repo[0].node(), revlog.nullid, )
        def filectxfn(repo, memctx, path):
            return context.memfilectx(path=path,
                                      data='added',
                                      islink=False,
                                      isexec=False,
                                      copied=False)
        ctx = context.memctx(self.repo,
                             parents,
                             'automated test',
                             ['added_bogus_file', 'other_added_file', ],
                             filectxfn,
                             'testy',
                             '2008-12-21 16:32:00 -0500',
                             {'branch': 'localbranch', })
        self.repo.commitctx(ctx)
        self.assertEqual(self.repo['tip'].branch(), 'localbranch')
        beforerebasehash = self.repo['tip'].node()
        hg.update(self.repo, 'tip')
        wrappers.rebase(rebase.rebase, self.ui(), self.repo, svn=True)
        self.assertEqual(self.repo['tip'].branch(), 'localbranch')
        self.assertEqual(self.repo['tip'].parents()[0].parents()[0], self.repo[0])
        self.assertNotEqual(beforerebasehash, self.repo['tip'].node())

    def test_genignore(self):
        """ Test generation of .hgignore file. """
        test_util.load_fixture_and_fetch('ignores.svndump', self.repo_path,
                                         self.wc_path, noupdate=False)
        u = self.ui()
        u.pushbuffer()
        svncommands.genignore(u, self.repo, self.wc_path)
        self.assertMultiLineEqual(open(os.path.join(self.wc_path, '.hgignore')).read(),
                         '.hgignore\nsyntax:glob\nblah\notherblah\nbaz/magic\n')

    def test_genignore_single(self):
        self._load_fixture_and_fetch('ignores.svndump', subdir='trunk')
        hg.update(self.repo, 'tip')
        u = self.ui()
        u.pushbuffer()
        svncommands.genignore(u, self.repo, self.wc_path)
        self.assertMultiLineEqual(open(os.path.join(self.wc_path, '.hgignore')).read(),
                               '.hgignore\nsyntax:glob\nblah\notherblah\nbaz/magic\n')

    def test_list_authors(self):
        test_util.load_svndump_fixture(self.repo_path,
                                       'replace_trunk_with_branch.svndump')
        u = self.ui()
        u.pushbuffer()
        svncommands.listauthors(u,
                                     args=[test_util.fileurl(self.repo_path)],
                                     authors=None)
        actual = u.popbuffer()
        self.assertMultiLineEqual(actual, 'Augie\nevil\n')


    def test_list_authors_map(self):
        test_util.load_svndump_fixture(self.repo_path,
                                       'replace_trunk_with_branch.svndump')
        author_path = os.path.join(self.repo_path, 'authors')
        svncommands.listauthors(self.ui(),
                                     args=[test_util.fileurl(self.repo_path)],
                                     authors=author_path)
        self.assertMultiLineEqual(open(author_path).read(), 'Augie=\nevil=\n')


def suite():
    all = [unittest.TestLoader().loadTestsFromTestCase(UtilityTests),
          ]
    return unittest.TestSuite(all)
