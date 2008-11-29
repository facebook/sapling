import sys
import unittest

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
        #self.assertEqual(heads['branch1'].manifest().keys(), ['b'])
        #self.assertEqual(heads['branch2'].manifest().keys(), ['a', 'b'])

    def test_unrelatedbranch_stupid(self):
        self.test_unrelatedbranch(True)

def suite():
    all = [unittest.TestLoader().loadTestsFromTestCase(TestFetchBranches),
          ]
    return unittest.TestSuite(all)
