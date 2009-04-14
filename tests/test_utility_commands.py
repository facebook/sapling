import os
import unittest

from hgext import rebase
from mercurial import ui
from mercurial import hg
from mercurial import revlog
from mercurial import context
from mercurial import node

import utility_commands
import test_util
import wrappers

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
    def test_info_output(self):
        self._load_fixture_and_fetch('two_heads.svndump')
        hg.update(self.repo, 'the_branch')
        u = ui.ui()
        utility_commands.info(u, self.repo, self.wc_path)
        expected = (expected_info_output %
                    {'date': '2008-10-08 01:39:05 +0000 (Wed, 08 Oct 2008)',
                     'repourl': test_util.fileurl(self.repo_path),
                     'branch': 'branches/the_branch',
                     'rev': 5,
                     })
        self.assertEqual(u.stream.getvalue(), expected)
        hg.update(self.repo, 'default')
        u = ui.ui()
        utility_commands.info(u, self.repo, self.wc_path)
        expected = (expected_info_output %
                    {'date': '2008-10-08 01:39:29 +0000 (Wed, 08 Oct 2008)',
                     'repourl': test_util.fileurl(self.repo_path),
                     'branch': 'trunk',
                     'rev': 6,
                     })
        self.assertEqual(u.stream.getvalue(), expected)

    def test_parent_output(self):
        self._load_fixture_and_fetch('two_heads.svndump')
        u = ui.ui()
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
        wrappers.parent(lambda x, y: None, u, self.repo, svn=True)
        self.assertEqual(u.stream.getvalue(),
                         'changeset:   3:4e256962fc5d\n'
                         'branch:      the_branch\n'
                         'user:        durin@df2126f7-00ab-4d49-b42c-7e981dde0bcf\n'
                         'date:        Wed Oct 08 01:39:05 2008 +0000\n'
                         'summary:     add delta on the branch\n\n')

        hg.update(self.repo, 'default')
        # Make sure styles work
        u = ui.ui()
        wrappers.parent(lambda x, y: None, u, self.repo, svn=True, style='compact')
        self.assertEqual(u.stream.getvalue(),
                         '4:1   1083037b18d8   2008-10-08 01:39 +0000   durin\n'
                         '  Add gamma on trunk.\n\n')
        # custom templates too
        u = ui.ui()
        wrappers.parent(lambda x, y: None, u, self.repo, svn=True, template='{node}\n')
        self.assertEqual(u.stream.getvalue(), '1083037b18d85cd84fa211c5adbaeff0fea2cd9f\n')

        u = ui.ui()
        wrappers.parent(lambda x, y: None, u, self.repo, svn=True)
        self.assertEqual(u.stream.getvalue(),
                         'changeset:   4:1083037b18d8\n'
                         'parent:      1:c95251e0dd04\n'
                         'user:        durin@df2126f7-00ab-4d49-b42c-7e981dde0bcf\n'
                         'date:        Wed Oct 08 01:39:29 2008 +0000\n'
                         'summary:     Add gamma on trunk.\n\n')

    def test_outgoing_output(self):
        self._load_fixture_and_fetch('two_heads.svndump')
        u = ui.ui()
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
        wrappers.outgoing(lambda x,y,z: None, u, self.repo, svn=True)
        self.assert_(node.hex(self.repo['localbranch'].node())[:8] in
                     u.stream.getvalue())
        self.assertEqual(u.stream.getvalue(), ('changeset:   5:6de15430fa20\n'
                                               'branch:      localbranch\n'
                                               'tag:         tip\n'
                                               'parent:      3:4e256962fc5d\n'
                                               'user:        testy\n'
                                               'date:        Sun Dec 21 16:32:00 2008 -0500\n'
                                               'summary:     automated test\n'
                                               '\n'))
        hg.update(self.repo, 'default')
        u = ui.ui()
        wrappers.outgoing(lambda x,y,z: None, u, self.repo, svn=True)
        self.assertEqual(u.stream.getvalue(), 'no changes found\n')

    def test_url_output(self):
        self._load_fixture_and_fetch('two_revs.svndump')
        hg.update(self.repo, 'tip')
        u = ui.ui()
        utility_commands.url(u, self.repo, self.wc_path)
        expected = test_util.fileurl(self.repo_path) + '\n'
        self.assertEqual(u.stream.getvalue(), expected)

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
        wrappers.rebase(rebase.rebase, ui.ui(), self.repo, svn=True)
        self.assertEqual(self.repo['tip'].branch(), 'localbranch')
        self.assertEqual(self.repo['tip'].parents()[0].parents()[0], self.repo[0])
        self.assertNotEqual(beforerebasehash, self.repo['tip'].node())

    def test_url_is_normalized(self):
        """Verify url gets normalized on initial clone.
        """
        test_util.load_svndump_fixture(self.repo_path, 'two_revs.svndump')
        wrappers.clone(None, ui.ui(),
                       source=test_util.fileurl(self.repo_path) + '/',
                       dest=self.wc_path, stupid=False)
        hg.update(self.repo, 'tip')
        u = ui.ui()
        utility_commands.url(u, self.repo, self.wc_path)
        expected = test_util.fileurl(self.repo_path) + '\n'
        self.assertEqual(u.stream.getvalue(), expected)

    def test_genignore(self):
        """Verify url gets normalized on initial clone.
        """
        test_util.load_svndump_fixture(self.repo_path, 'ignores.svndump')
        wrappers.clone(None, ui.ui(),
                       source=test_util.fileurl(self.repo_path) + '/',
                       dest=self.wc_path, stupid=False)
        hg.update(self.repo, 'tip')
        u = ui.ui()
        utility_commands.genignore(u, self.repo, self.wc_path)
        self.assertEqual(open(os.path.join(self.wc_path, '.hgignore')).read(),
                         '.hgignore\nsyntax:glob\nblah\notherblah\nbaz/magic\n')

    def test_list_authors(self):
        test_util.load_svndump_fixture(self.repo_path,
                                       'replace_trunk_with_branch.svndump')
        u = ui.ui()
        utility_commands.listauthors(u,
                                     args=[test_util.fileurl(self.repo_path)],
                                     authors=None)
        self.assertEqual(u.stream.getvalue(), 'Augie\nevil\n')


    def test_list_authors_map(self):
        test_util.load_svndump_fixture(self.repo_path,
                                       'replace_trunk_with_branch.svndump')
        author_path = os.path.join(self.repo_path, 'authors')
        utility_commands.listauthors(ui.ui(),
                                     args=[test_util.fileurl(self.repo_path)],
                                     authors=author_path)
        self.assertEqual(open(author_path).read(), 'Augie=\nevil=\n')


def suite():
    all = [unittest.TestLoader().loadTestsFromTestCase(UtilityTests),
          ]
    return unittest.TestSuite(all)
