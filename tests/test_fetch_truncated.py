import unittest

from mercurial import hg
from mercurial import ui

import svncommands
import test_util

class TestFetchTruncatedHistory(test_util.TestBase):
    def test_truncated_history(self, stupid=False):
        # Test repository does not follow the usual layout
        test_util.load_svndump_fixture(self.repo_path, 'truncatedhistory.svndump')
        svn_url = test_util.fileurl(self.repo_path + '/project2')
        svncommands.pull(ui.ui(), svn_url=svn_url,
                         hg_repo_path=self.wc_path, stupid=stupid)
        repo = hg.repository(ui.ui(), self.wc_path)

        # We are converting /project2/trunk coming from:
        #
        # Changed paths:
        #     D /project1
        #     A /project2/trunk (from /project1:2)
        #
        # Here a full fetch should be performed since we are starting
        # the conversion on an already filled branch.
        tip = repo['tip']
        files = tip.manifest().keys()
        files.sort()
        self.assertEqual(files, ['a', 'b'])
        self.assertEqual(repo['tip']['a'].data(), 'a\n')

    def test_truncated_history_stupid(self):
        self.test_truncated_history(True)

def suite():
    all = [unittest.TestLoader().loadTestsFromTestCase(TestFetchTruncatedHistory),
          ]
    return unittest.TestSuite(all)
