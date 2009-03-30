import os
import unittest

from mercurial import ui
from mercurial import hg
from mercurial import revlog
from mercurial import context
from mercurial import node

import utility_commands
import fetch_command
import test_util

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
        utility_commands.run_svn_info(u, self.repo, self.wc_path)
        expected = (expected_info_output %
                    {'date': '2008-10-08 01:39:05 +0000 (Wed, 08 Oct 2008)',
                     'repourl': test_util.fileurl(self.repo_path),
                     'branch': 'branches/the_branch',
                     'rev': 5,
                     })
        self.assertEqual(u.stream.getvalue(), expected)
        hg.update(self.repo, 'default')
        u = ui.ui()
        utility_commands.run_svn_info(u, self.repo, self.wc_path)
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
        utility_commands.print_parent_revision(u, self.repo, self.wc_path)
        self.assert_(node.hex(self.repo['the_branch'].node())[:8] in
                     u.stream.getvalue())
        self.assert_('the_branch' in u.stream.getvalue())
        self.assert_('r5' in u.stream.getvalue())
        hg.update(self.repo, 'default')
        u = ui.ui()
        utility_commands.print_parent_revision(u, self.repo, self.wc_path)
        self.assert_(node.hex(self.repo['default'].node())[:8] in
                     u.stream.getvalue())
        self.assert_('trunk' in u.stream.getvalue())
        self.assert_('r6' in u.stream.getvalue())

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
        utility_commands.show_outgoing_to_svn(u, self.repo, self.wc_path)
        self.assert_(node.hex(self.repo['localbranch'].node())[:8] in
                     u.stream.getvalue())
        self.assert_('testy' in u.stream.getvalue())
        hg.update(self.repo, 'default')
        u = ui.ui()
        utility_commands.show_outgoing_to_svn(u, self.repo, self.wc_path)
        self.assertEqual(u.stream.getvalue(), 'No outgoing changes found.\n')

    def test_url_output(self):
        self._load_fixture_and_fetch('two_revs.svndump')
        hg.update(self.repo, 'tip')
        u = ui.ui()
        utility_commands.print_wc_url(u, self.repo, self.wc_path)
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
        utility_commands.rebase_commits(ui.ui(), self.repo)
        self.assertEqual(self.repo['tip'].branch(), 'localbranch')
        self.assertEqual(self.repo['tip'].parents()[0].parents()[0], self.repo[0])
        self.assertNotEqual(beforerebasehash, self.repo['tip'].node())

    def test_url_is_normalized(self):
        """Verify url gets normalized on initial clone.
        """
        test_util.load_svndump_fixture(self.repo_path, 'two_revs.svndump')
        fetch_command.fetch_revisions(ui.ui(),
                                      svn_url=test_util.fileurl(self.repo_path)+'/',
                                      hg_repo_path=self.wc_path,
                                      stupid=False)
        hg.update(self.repo, 'tip')
        u = ui.ui()
        utility_commands.print_wc_url(u, self.repo, self.wc_path)
        expected = test_util.fileurl(self.repo_path) + '\n'
        self.assertEqual(u.stream.getvalue(), expected)

    def test_genignore(self):
        """Verify url gets normalized on initial clone.
        """
        test_util.load_svndump_fixture(self.repo_path, 'ignores.svndump')
        fetch_command.fetch_revisions(ui.ui(),
                                      svn_url=test_util.fileurl(self.repo_path)+'/',
                                      hg_repo_path=self.wc_path,
                                      stupid=False)
        hg.update(self.repo, 'tip')
        u = ui.ui()
        utility_commands.generate_ignore(u, self.repo, self.wc_path)
        self.assertEqual(open(os.path.join(self.wc_path, '.hgignore')).read(),
                         '.hgignore\nsyntax:glob\nblah\notherblah\nbaz/magic\n')


def suite():
    all = [unittest.TestLoader().loadTestsFromTestCase(UtilityTests),
          ]
    return unittest.TestSuite(all)
