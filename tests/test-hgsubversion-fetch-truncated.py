import test_hgsubversion_util
from edenscm.mercurial import commands, hg


class TestFetchTruncatedHistory(test_hgsubversion_util.TestBase):
    stupid_mode_tests = True

    def test_truncated_history(self):
        # Test repository does not follow the usual layout
        repo_path = self.load_svndump("truncatedhistory.svndump")
        svn_url = test_hgsubversion_util.fileurl(repo_path + "/project2")
        commands.clone(self.ui(), svn_url, self.wc_path, noupdate=True)
        repo = hg.repository(self.ui(), self.wc_path)

        # We are converting /project2/trunk coming from:
        #
        # Changed paths:
        #     D /project1
        #     A /project2/trunk (from /project1:2)
        #
        # Here a full fetch should be performed since we are starting
        # the conversion on an already filled branch.
        tip = repo["tip"]
        files = tip.manifest().keys()
        files.sort()
        self.assertEqual(files, ["a", "b"])
        self.assertEqual(repo["tip"]["a"].data(), "a\n")


if __name__ == "__main__":
    import silenttestrunner

    silenttestrunner.main(__name__)
