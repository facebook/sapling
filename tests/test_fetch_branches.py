import sys
import unittest

from mercurial import node

import test_util


class TestFetchBranches(test_util.TestBase):
    def _load_fixture_and_fetch(self, fixture_name, stupid):
        return test_util.load_fixture_and_fetch(fixture_name, self.repo_path,
                                                self.wc_path, stupid=stupid)

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
        self.assertEqual(node.hex(repo['tip'].node()),
                         'b479347c1f56d1fafe5e32a7ce0d1b7099637784')
        self.assertEqual(repo['tip'].parents()[0].branch(), 'dev_branch')
        self.assertEqual(repo['old_trunk'].parents()[0].branch(), 'default')

    def test_renamed_branch_to_trunk_stupid(self):
        self.test_renamed_branch_to_trunk(stupid=True)

def suite():
    all = [unittest.TestLoader().loadTestsFromTestCase(TestFetchBranches),
          ]
    return unittest.TestSuite(all)
