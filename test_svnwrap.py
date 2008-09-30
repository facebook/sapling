import os
import shutil
import tempfile
import unittest

from nose import tools

import svnwrap

class TestBasicRepoLayout(unittest.TestCase):
    def setUp(self):
        self.oldwd = os.getcwd()
        self.tmpdir = tempfile.mkdtemp('svnwrap_test')
        self.repo_path = '%s/testrepo' % self.tmpdir
        wc_path = '%s/testrepo_wc' % self.tmpdir
        os.spawnvp(os.P_WAIT, 'svnadmin', ['svnadmin', 'create',
                                           self.repo_path,])
        os.spawnvp(os.P_WAIT, 'svn', ['svn', 'checkout',
                                      'file://%s' % self.repo_path,
                                      wc_path,])
        os.chdir(wc_path)
        for d in ['branches', 'tags', 'trunk']:
            os.mkdir(os.path.join(wc_path, d))
        #r1
        os.spawnvp(os.P_WAIT, 'svn', ['svn', 'add', 'branches', 'tags', 'trunk'])
        os.spawnvp(os.P_WAIT, 'svn', ['svn', 'ci', '-m', 'Empty dirs.'])
        #r2
        files = ['alpha', 'beta', 'delta']
        for f in files:
            open(os.path.join(wc_path, 'trunk', f), 'w').write('This is %s.\n' % f)
        os.chdir('trunk')
        os.spawnvp(os.P_WAIT, 'svn', ['svn', 'add']+files)
        os.spawnvp(os.P_WAIT, 'svn', ['svn', 'ci', '-m', 'Initial Files.'])
        os.chdir('..')
        #r3
        os.spawnvp(os.P_WAIT, 'svn', ['svn', 'cp', 'trunk', 'tags/rev1'])
        os.spawnvp(os.P_WAIT, 'svn', ['svn', 'ci', '-m', 'Tag rev 1.'])
        #r4
        os.spawnvp(os.P_WAIT, 'svn', ['svn', 'cp', 'trunk', 'branches/crazy'])
        os.spawnvp(os.P_WAIT, 'svn', ['svn', 'ci', '-m', 'Branch to crazy.'])

        #r5
        open(os.path.join(wc_path, 'trunk', 'gamma'), 'w').write('This is %s.\n'
                                                                 % 'gamma')
        os.spawnvp(os.P_WAIT, 'svn', ['svn', 'add', 'trunk/gamma', ])
        os.spawnvp(os.P_WAIT, 'svn', ['svn', 'ci', '-m', 'Add gamma'])

        #r6
        open(os.path.join(wc_path, 'branches', 'crazy', 'omega'),
             'w').write('This is %s.\n' % 'omega')
        os.spawnvp(os.P_WAIT, 'svn', ['svn', 'add', 'branches/crazy/omega', ])
        os.spawnvp(os.P_WAIT, 'svn', ['svn', 'ci', '-m', 'Add omega'])

        #r7
        os.spawnvp(os.P_WAIT, 'svn', ['svn', 'cp', 'trunk', 'branches/more_crazy'])
        os.spawnvp(os.P_WAIT, 'svn', ['svn', 'ci', '-m', 'Branch to more_crazy.'])

        self.repo = svnwrap.SubversionRepo('file://%s' % self.repo_path)

    def tearDown(self):
        shutil.rmtree(self.tmpdir)
        os.chdir(self.oldwd)


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
        self.oldwd = os.getcwd()
        self.tmpdir = tempfile.mkdtemp('svnwrap_test')
        self.repo_path = '%s/testrepo' % self.tmpdir
        wc_path = '%s/testrepo_wc' % self.tmpdir
        os.spawnvp(os.P_WAIT, 'svnadmin', ['svnadmin', 'create',
                                           self.repo_path,])
        os.spawnvp(os.P_WAIT, 'svn', ['svn', 'checkout',
                                      'file://%s' % self.repo_path,
                                      wc_path,])
        self.repo_path += '/dummyproj'
        os.chdir(wc_path)
        os.mkdir('dummyproj')
        os.chdir('dummyproj')
        wc_path += '/dummyproj'
        for d in ['branches', 'tags', 'trunk']:
            os.mkdir(os.path.join(wc_path, d))
        #r1
        os.chdir('..')
        os.spawnvp(os.P_WAIT, 'svn', ['svn', 'add', 'dummyproj'])
        os.spawnvp(os.P_WAIT, 'svn', ['svn', 'ci', '-m', 'Empty dirs.'])
        os.chdir('dummyproj')
        #r2
        files = ['alpha', 'beta', 'delta']
        for f in files:
            open(os.path.join(wc_path, 'trunk', f), 'w').write('This is %s.\n' % f)
        os.chdir('trunk')
        os.spawnvp(os.P_WAIT, 'svn', ['svn', 'add']+files)
        os.spawnvp(os.P_WAIT, 'svn', ['svn', 'ci', '-m', 'Initial Files.'])
        os.chdir('..')
        #r3
        os.spawnvp(os.P_WAIT, 'svn', ['svn', 'cp', 'trunk', 'tags/rev1'])
        os.spawnvp(os.P_WAIT, 'svn', ['svn', 'ci', '-m', 'Tag rev 1.'])
        #r4
        os.spawnvp(os.P_WAIT, 'svn', ['svn', 'cp', 'trunk', 'branches/crazy'])
        os.spawnvp(os.P_WAIT, 'svn', ['svn', 'ci', '-m', 'Branch to crazy.'])

        #r5
        open(os.path.join(wc_path, 'trunk', 'gamma'), 'w').write('This is %s.\n'
                                                                 % 'gamma')
        os.spawnvp(os.P_WAIT, 'svn', ['svn', 'add', 'trunk/gamma', ])
        os.spawnvp(os.P_WAIT, 'svn', ['svn', 'ci', '-m', 'Add gamma'])

        #r6
        open(os.path.join(wc_path, 'branches', 'crazy', 'omega'),
             'w').write('This is %s.\n' % 'omega')
        os.spawnvp(os.P_WAIT, 'svn', ['svn', 'add', 'branches/crazy/omega', ])
        os.spawnvp(os.P_WAIT, 'svn', ['svn', 'ci', '-m', 'Add omega'])

        #r7
        os.spawnvp(os.P_WAIT, 'svn', ['svn', 'cp', 'trunk', 'branches/more_crazy'])
        os.spawnvp(os.P_WAIT, 'svn', ['svn', 'ci', '-m', 'Branch to more_crazy.'])

        self.repo = svnwrap.SubversionRepo('file://%s' % (self.repo_path))
