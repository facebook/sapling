import unittest

from mercurial import hg
from mercurial import node
from mercurial import ui

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
        repo = hg.clone(ui.ui(), source=source, dest=self.wc_path)
        return hg.repository(ui.ui(), self.wc_path)

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

    def test_renamed_branch_to_trunk_stupid(self):
        self.test_renamed_branch_to_trunk(stupid=True)

    def test_replace_trunk_with_branch(self, stupid=False):
        repo = self._load_fixture_and_fetch('replace_trunk_with_branch.svndump',
                                            stupid)
        self.assertEqual(repo['default'].parents()[0].branch(), 'test')
        self.assertEqual(node.hex(repo['closed-branches'].parents()[0].node()),
                         '2cd09772e0f6ddf2d13c60ef3c1be11ad5a7dfae')
        self.assertEqual(node.hex(repo['default'].node()),
                         '8a525ca0671f456e6b1417187bf86c6115d2cb78')

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
    
    def test_branch_tip_update_to_branch_anchor(self):
        repo = self._load_fixture_and_fetch_with_anchor(
            'unorderedbranch.svndump', 'branch')
        self.assertEqual(repo[None].branch(), 'branch')
        self.assertEqual(repo[None].parents()[0], repo[repo.branchheads()[0]])

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

def suite():
    all = [unittest.TestLoader().loadTestsFromTestCase(TestFetchBranches),
          ]
    return unittest.TestSuite(all)
