import test_util

import unittest

class TestFetchBinaryFiles(test_util.TestBase):
    def test_binaryfiles(self, stupid=False):
        repo = self._load_fixture_and_fetch('binaryfiles.svndump', stupid=stupid)
        self.assertEqual('cce7fe400d8d', str(repo['tip']))

    def test_binaryfiles_stupid(self):
        self.test_binaryfiles(True)

def suite():
    all = [unittest.TestLoader().loadTestsFromTestCase(TestFetchBinaryFiles),
          ]
    return unittest.TestSuite(all)
