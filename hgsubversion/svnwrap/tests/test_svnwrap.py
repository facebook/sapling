import os
import subprocess
import shutil
import tempfile
import unittest

from nose import tools

import svnwrap

class TestBasicRepoLayout(unittest.TestCase):
    def setUp(self):
        self.tmpdir = tempfile.mkdtemp('svnwrap_test')
        self.repo_path = '%s/testrepo' % self.tmpdir
        os.spawnvp(os.P_WAIT, 'svnadmin', ['svnadmin', 'create',
                                           self.repo_path,])
        inp = open(os.path.join(os.path.dirname(__file__), 'fixtures',
                                'project_root_at_repo_root.svndump'))
        proc = subprocess.call(['svnadmin', 'load', self.repo_path,],
                                stdin=inp, close_fds=True,
                                stdout=subprocess.PIPE,
                                stderr=subprocess.STDOUT)
        assert proc == 0
        self.repo = svnwrap.SubversionRepo('file://%s' % self.repo_path)

    def tearDown(self):
        shutil.rmtree(self.tmpdir)


    def test_num_revs(self):
        revs = list(self.repo.revisions())
        tools.eq_(len(revs), 7)
        r = revs[1]
        tools.eq_(r.revnum, 2)
        tools.eq_(sorted(r.paths.keys()),
                  ['trunk/alpha', 'trunk/beta', 'trunk/delta'])
        for r in revs:
            for p in r.paths:
                # make sure these paths are always non-absolute for sanity
                if p:
                    assert p[0] != '/'
        revs = list(self.repo.revisions(start=3))
        tools.eq_(len(revs), 4)


    def test_branches(self):
        tools.eq_(self.repo.branches.keys(), ['crazy', 'more_crazy'])
        tools.eq_(self.repo.branches['crazy'], ('trunk', 2, 4))
        tools.eq_(self.repo.branches['more_crazy'], ('trunk', 5, 7))


    def test_tags(self):
        tags = self.repo.tags
        tools.eq_(tags.keys(), ['rev1'])
        tools.eq_(tags['rev1'], ('trunk', 2))

class TestRootAsSubdirOfRepo(TestBasicRepoLayout):
    def setUp(self):
        self.tmpdir = tempfile.mkdtemp('svnwrap_test')
        self.repo_path = '%s/testrepo' % self.tmpdir
        os.spawnvp(os.P_WAIT, 'svnadmin', ['svnadmin', 'create',
                                           self.repo_path,])
        inp = open(os.path.join(os.path.dirname(__file__), 'fixtures',
                                'project_root_not_repo_root.svndump'))
        ret = subprocess.call(['svnadmin', 'load', self.repo_path,],
                              stdin=inp, close_fds=True,
                              stdout=subprocess.PIPE,
                              stderr=subprocess.STDOUT)
        assert ret == 0
        self.repo = svnwrap.SubversionRepo('file://%s/dummyproj' %
                                           self.repo_path)
