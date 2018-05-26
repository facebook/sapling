# no-check-code -- see T24862348

import os
import subprocess
import tempfile
import unittest

import test_hgsubversion_util
from hgext.hgsubversion import svnwrap


class TestBasicRepoLayout(unittest.TestCase):

    def setUp(self):
        self.tmpdir = tempfile.mkdtemp("svnwrap_test")
        self.repo_path = "%s/testrepo" % self.tmpdir

        with open(
            os.path.join(
                test_hgsubversion_util.FIXTURES, "project_root_at_repo_root.svndump"
            )
        ) as fp:
            svnwrap.create_and_load(self.repo_path, fp)

        self.repo = svnwrap.SubversionRepo(
            test_hgsubversion_util.fileurl(self.repo_path)
        )

    def tearDown(self):
        del self.repo
        test_hgsubversion_util.rmtree(self.tmpdir)

    def test_num_revs(self):
        revs = list(self.repo.revisions())
        self.assertEqual(len(revs), 7)
        r = revs[1]
        self.assertEqual(r.revnum, 2)
        self.assertEqual(
            sorted(r.paths.keys()), ["trunk/alpha", "trunk/beta", "trunk/delta"]
        )
        for r in revs:
            for p in r.paths:
                # make sure these paths are always non-absolute for sanity
                if p:
                    assert p[0] != "/"
        revs = list(self.repo.revisions(start=3))
        self.assertEqual(len(revs), 4)


class TestRootAsSubdirOfRepo(TestBasicRepoLayout):

    def setUp(self):
        self.tmpdir = tempfile.mkdtemp("svnwrap_test")
        self.repo_path = "%s/testrepo" % self.tmpdir

        with open(
            os.path.join(
                test_hgsubversion_util.FIXTURES, "project_root_not_repo_root.svndump"
            )
        ) as fp:
            svnwrap.create_and_load(self.repo_path, fp)

        self.repo = svnwrap.SubversionRepo(
            test_hgsubversion_util.fileurl(self.repo_path + "/dummyproj")
        )


if __name__ == "__main__":
    import silenttestrunner

    silenttestrunner.main(__name__)
