import unittest

from mercurial import hg
from mercurial import node
from mercurial import util as hgutil

import test_util


class TestFetchBranches(test_util.TestBase):
    def _load_fixture_and_fetch(self, fixture_name, stupid, noupdate=True,
                                subdir=''):
        return test_util.load_fixture_and_fetch(fixture_name, self.repo_path,
                                                self.wc_path, stupid=stupid,
                                                noupdate=noupdate, subdir=subdir)

    def _load_fixture_and_fetch_with_anchor(self, fixture_name, anchor):
        test_util.load_svndump_fixture(self.repo_path, fixture_name)
        source = '%s#%s' % (test_util.fileurl(self.repo_path), anchor)
        repo = hg.clone(self.ui(), source=source, dest=self.wc_path)
        return hg.repository(self.ui(), self.wc_path)

    def openbranches(self, repo):
        hctxs = [repo[hn] for hn in repo.heads()]
        branches = set(ctx.branch() for ctx in hctxs if
                       ctx.extra().get('close', None) != '1')
        return sorted(branches)

    def test_rename_branch_parent(self, stupid=False):
        repo = self._load_fixture_and_fetch('rename_branch_parent_dir.svndump', stupid)
        heads = [repo[n] for n in repo.heads()]
        heads = dict([(ctx.branch(), ctx) for ctx in heads])
        # Let these tests disabled yet as the fix is not obvious
        self.assertEqual(['dev_branch'], self.openbranches(repo))

    def test_rename_branch_parent_stupid(self):
        self.test_rename_branch_parent(stupid=True)

    def test_unrelatedbranch(self, stupid=False):
        repo = self._load_fixture_and_fetch('unrelatedbranch.svndump', stupid)
        heads = [repo[n] for n in repo.heads()]
        heads = dict([(ctx.branch(), ctx) for ctx in heads])
        # Let these tests disabled yet as the fix is not obvious
        self.assertEqual(heads['branch1'].manifest().keys(), ['b'])
        self.assertEqual(heads['branch2'].manifest().keys(), ['a', 'b'])

    def test_unrelatedbranch_stupid(self):
        self.test_unrelatedbranch(True)

    def test_unorderedbranch(self, stupid=False):
        repo = self._load_fixture_and_fetch('unorderedbranch.svndump', stupid)
        r = repo['branch']
        self.assertEqual(0, r.parents()[0].rev())
        self.assertEqual(['a', 'c', 'z'], sorted(r.manifest()))

    def test_unorderedbranch_stupid(self):
        self.test_unorderedbranch(True)

    def test_renamed_branch_to_trunk(self, stupid=False):
        repo = self._load_fixture_and_fetch('branch_rename_to_trunk.svndump',
                                            stupid)
        self.assertEqual(node.hex(repo['default'].node()),
                         '14d252aef315857df241dd3fa4bc7833b09bd2f5')
        self.assertEqual(repo['default'].parents()[0].branch(), 'dev_branch')
        self.assertEqual(repo['old_trunk'].parents()[0].branch(), 'default')
        expected = ['default', 'old_trunk']
        self.assertEqual(self.openbranches(repo), expected)

    def test_renamed_branch_to_trunk_stupid(self):
        self.test_renamed_branch_to_trunk(stupid=True)

    def test_replace_trunk_with_branch(self, stupid=False):
        repo = self._load_fixture_and_fetch('replace_trunk_with_branch.svndump',
                                            stupid)
        self.assertEqual(repo['default'].parents()[0].branch(), 'test')
        self.assertEqual(repo['tip'].branch(), 'default')
        self.assertEqual(repo['tip'].extra().get('close'), '1')
        self.assertEqual(self.openbranches(repo), ['default'])

    def test_copybeforeclose(self, stupid=False):
        repo = self._load_fixture_and_fetch('copybeforeclose.svndump', stupid)
        self.assertEqual(repo['tip'].branch(), 'test')
        self.assertEqual(repo['test'].extra().get('close'), '1')
        self.assertEqual(repo['test']['b'].data(), 'a\n')

    def test_copybeforeclose_stupid(self):
        self.test_copybeforeclose(True)

    def test_replace_trunk_with_branch_stupid(self):
        self.test_replace_trunk_with_branch(stupid=True)

    def test_branch_create_with_dir_delete_works(self, stupid=False):
        repo = self._load_fixture_and_fetch('branch_create_with_dir_delete.svndump',
                                            stupid)
        self.assertEqual(repo['tip'].manifest().keys(),
                         ['alpha', 'beta', 'iota', 'gamma', ])

    def test_branch_tip_update_to_default(self, stupid=False):
        repo = self._load_fixture_and_fetch('unorderedbranch.svndump',
                                            stupid, noupdate=False)
        self.assertEqual(repo[None].branch(), 'default')
        self.assertTrue('tip' not in repo[None].tags())

    def test_branch_tip_update_to_default_stupid(self):
        self.test_branch_tip_update_to_default(True)

    def test_branch_pull_anchor(self):
        self.assertRaises(hgutil.Abort,
                          self._load_fixture_and_fetch_with_anchor,
                          'unorderedbranch.svndump', 'NaN')
        repo = self._load_fixture_and_fetch_with_anchor(
            'unorderedbranch.svndump', '4')
        self.assertTrue('c' not in repo.branchtags())

    def test_branches_weird_moves(self, stupid=False):
        repo = self._load_fixture_and_fetch('renamedproject.svndump', stupid,
                                            subdir='project')
        heads = [repo[n] for n in repo.heads()]
        heads = dict((ctx.branch(), ctx) for ctx in heads)
        mdefault = sorted(heads['default'].manifest().keys())
        mbranch = sorted(heads['branch'].manifest().keys())
        self.assertEqual(mdefault, ['a', 'b', 'd/a'])
        self.assertEqual(mbranch, ['a'])

    def test_branches_weird_moves_stupid(self):
        self.test_branches_weird_moves(True)

    def test_branch_delete_parent_dir(self, stupid=False):
        repo = self._load_fixture_and_fetch('branch_delete_parent_dir.svndump',
                                            stupid)
        self.assertEqual(node.hex(repo['tip'].node()),
                         '4108a81a82c7925d5551091165dc54c41b06a8a8')

    def test_replace_branch_with_branch(self, stupid=False):
        repo = self._load_fixture_and_fetch('replace_branch_with_branch.svndump',
                                            stupid)
        self.assertEqual(7, len(repo))
        # tip is former topological branch1 being closed
        ctx = repo['tip']
        self.assertEqual('1', ctx.extra().get('close', '0'))
        self.assertEqual('branch1', ctx.branch())
        # r5 is where the replacement takes place
        ctx = repo[5]
        self.assertEqual(set(['a', 'c']), set(ctx))
        self.assertEqual('0', ctx.extra().get('close', '0'))
        self.assertEqual('branch1', ctx.branch())
        self.assertEqual('c\n', ctx['c'].data())
        self.assertEqual('d\n', ctx['a'].data())

    def test_replace_branch_with_branch_stupid(self, stupid=False):
        self.test_replace_branch_with_branch(True)

def suite():
    all = [unittest.TestLoader().loadTestsFromTestCase(TestFetchBranches),
          ]
    return unittest.TestSuite(all)
