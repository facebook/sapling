import test_util

import unittest

from mercurial import error
from mercurial import hg
from mercurial import node

class TestFetchBranches(test_util.TestBase):
    stupid_mode_tests = True

    def _load_fixture_and_fetch_with_anchor(self, fixture_name, anchor):
        repo_path = self.load_svndump(fixture_name)
        source = '%s#%s' % (test_util.fileurl(repo_path), anchor)
        test_util.hgclone(self.ui(), source, self.wc_path)
        return hg.repository(self.ui(), self.wc_path)

    def branches(self, repo):
        hctxs = [repo[hn] for hn in repo.heads()]
        openbranches = set(ctx.branch() for ctx in hctxs if
                           ctx.extra().get('close', None) != '1')
        closedbranches = set(ctx.branch() for ctx in hctxs if
                             ctx.extra().get('close', None) == '1')
        return sorted(openbranches), sorted(closedbranches)

    def openbranches(self, repo):
        return self.branches(repo)[0]

    def test_rename_branch_parent(self):
        repo = self._load_fixture_and_fetch('rename_branch_parent_dir.svndump')
        heads = [repo[n] for n in repo.heads()]
        heads = dict([(ctx.branch(), ctx) for ctx in heads])
        # Let these tests disabled yet as the fix is not obvious
        self.assertEqual(['dev_branch'], self.openbranches(repo))

    def test_unrelatedbranch(self):
        repo = self._load_fixture_and_fetch('unrelatedbranch.svndump')
        heads = [repo[n] for n in repo.heads()]
        heads = dict([(ctx.branch(), ctx) for ctx in heads])
        # Let these tests disabled yet as the fix is not obvious
        self.assertEqual(heads['branch1'].manifest().keys(), ['b'])
        self.assertEqual(heads['branch2'].manifest().keys(), ['a', 'b'])

    def test_unorderedbranch(self):
        repo = self._load_fixture_and_fetch('unorderedbranch.svndump')
        r = repo['branch']
        self.assertEqual(0, r.parents()[0].rev())
        self.assertEqual(['a', 'c', 'z'], sorted(r.manifest()))

    def test_renamed_branch_to_trunk(self):
        config = {'hgsubversion.failonmissing': 'true'}
        repo = self._load_fixture_and_fetch('branch_rename_to_trunk.svndump',
                                            config=config)
        self.assertEqual(repo['default'].parents()[0].branch(), 'dev_branch')
        self.assert_('iota' in repo['default'])
        self.assertEqual(repo['old_trunk'].parents()[0].branch(), 'default')
        self.assert_('iota' not in repo['old_trunk'])
        expected = ['default', 'old_trunk']
        self.assertEqual(self.openbranches(repo), expected)

    def test_replace_trunk_with_branch(self):
        repo = self._load_fixture_and_fetch('replace_trunk_with_branch.svndump')
        self.assertEqual(repo['default'].parents()[0].branch(), 'test')
        self.assertEqual(repo['tip'].branch(), 'default')
        self.assertEqual(repo['tip'].extra().get('close'), '1')
        self.assertEqual(self.openbranches(repo), ['default'])

    def test_copybeforeclose(self):
        repo = self._load_fixture_and_fetch('copybeforeclose.svndump')
        self.assertEqual(repo['tip'].branch(), 'test')
        self.assertEqual(repo['test'].extra().get('close'), '1')
        self.assertEqual(repo['test']['b'].data(), 'a\n')

    def test_branch_create_with_dir_delete_works(self):
        repo = self._load_fixture_and_fetch('branch_create_with_dir_delete.svndump')
        self.assertEqual(repo['tip'].manifest().keys(),
                         ['alpha', 'beta', 'iota', 'gamma', ])

    def test_branch_tip_update_to_default(self):
        repo = self._load_fixture_and_fetch('unorderedbranch.svndump',
                                            noupdate=False)
        self.assertEqual(repo[None].branch(), 'default')
        self.assertTrue('tip' not in repo[None].tags())

    def test_branch_pull_anchor(self):
        self.assertRaises(error.RepoLookupError,
                          self._load_fixture_and_fetch_with_anchor,
                          'unorderedbranch.svndump', 'NaN')
        repo = self._load_fixture_and_fetch_with_anchor(
            'unorderedbranch.svndump', '4')
        self.assertTrue('c' not in repo.branchtags())

    def test_branches_weird_moves(self):
        repo = self._load_fixture_and_fetch('renamedproject.svndump',
                                            subdir='project')
        heads = [repo[n] for n in repo.heads()]
        heads = dict((ctx.branch(), ctx) for ctx in heads)
        mdefault = sorted(heads['default'].manifest().keys())
        mbranch = sorted(heads['branch'].manifest().keys())
        self.assertEqual(mdefault, ['a', 'b', 'd/a'])
        self.assertEqual(mbranch, ['a'])

    def test_branch_delete_parent_dir(self):
        repo = self._load_fixture_and_fetch('branch_delete_parent_dir.svndump')
        openb, closedb = self.branches(repo)
        self.assertEqual(openb, [])
        self.assertEqual(closedb, ['dev_branch'])
        self.assertEqual(list(repo['dev_branch']), ['foo'])

    def test_replace_branch_with_branch(self):
        repo = self._load_fixture_and_fetch('replace_branch_with_branch.svndump')
        self.assertEqual(7, test_util.repolen(repo))
        # tip is former topological branch1 being closed
        ctx = repo['tip']
        self.assertEqual('1', ctx.extra().get('close', '0'))
        self.assertEqual('branch1', ctx.branch())
        # r5 is where the replacement takes place
        ctx = repo[5]
        self.assertEqual(set(['a', 'c', 'dir/e', 'dir2/e', 'f', 'g']), set(ctx))
        self.assertEqual('0', ctx.extra().get('close', '0'))
        self.assertEqual('branch1', ctx.branch())
        self.assertEqual('c\n', ctx['c'].data())
        self.assertEqual('d\n', ctx['a'].data())
        self.assertEqual('e\n', ctx['dir/e'].data())
        self.assertEqual('e\n', ctx['dir2/e'].data())
        self.assertEqual('f\n', ctx['f'].data())
        self.assertEqual('g\n', ctx['g'].data())
        for f in ctx:
            self.assertTrue(not ctx[f].renamed())

