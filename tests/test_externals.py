import os, unittest

from mercurial import commands

from hgsubversion import svnexternals
import test_util

class TestFetchExternals(test_util.TestBase):
    def test_externalsfile(self):
        f = svnexternals.externalsfile()
        f['t1'] = 'dir1 -r10 svn://foobar'
        f['t 2'] = 'dir2 -r10 svn://foobar'
        f['t3'] = ['dir31 -r10 svn://foobar', 'dir32 -r10 svn://foobar']

        refext = """[t 2]
 dir2 -r10 svn://foobar
[t1]
 dir1 -r10 svn://foobar
[t3]
 dir31 -r10 svn://foobar
 dir32 -r10 svn://foobar
"""
        value = f.write()
        self.assertEqual(refext, value)

        f2 = svnexternals.externalsfile()
        f2.read(value)
        self.assertEqual(sorted(f), sorted(f2))
        for t in f:
            self.assertEqual(f[t], f2[t])

    def test_parsedefinitions(self):
        # Taken from svn book
        samples = [
            ('third-party/sounds             http://svn.example.com/repos/sounds',
             ('third-party/sounds', None, 'http://svn.example.com/repos/sounds', None)),
            ('third-party/skins -r148        http://svn.example.com/skinproj',
             ('third-party/skins', '148', 'http://svn.example.com/skinproj', None)),
            ('third-party/skins -r 148        http://svn.example.com/skinproj',
             ('third-party/skins', '148', 'http://svn.example.com/skinproj', None)),
            ('http://svn.example.com/repos/sounds third-party/sounds',
             ('third-party/sounds', None, 'http://svn.example.com/repos/sounds', None)),
            ('-r148 http://svn.example.com/skinproj third-party/skins',
             ('third-party/skins', '148', 'http://svn.example.com/skinproj', None)),
            ('-r 148 http://svn.example.com/skinproj third-party/skins',
             ('third-party/skins', '148', 'http://svn.example.com/skinproj', None)),
            ('http://svn.example.com/skin-maker@21 third-party/skins/toolkit',
             ('third-party/skins/toolkit', None, 'http://svn.example.com/skin-maker', '21')),
            ]

        for line, expected in samples:
            self.assertEqual(expected, svnexternals.parsedefinition(line))

    def test_externals(self, stupid=False):
        repo = self._load_fixture_and_fetch('externals.svndump', stupid=stupid)

        ref0 = """[.]
 ^/externals/project1 deps/project1
"""
        self.assertEqual(ref0, repo[0]['.hgsvnexternals'].data())
        ref1 = """[.]
 ^/externals/project1 deps/project1
 -r2 ^/externals/project2@2 deps/project2
"""
        self.assertEqual(ref1, repo[1]['.hgsvnexternals'].data())

        ref2 = """[.]
 -r2 ^/externals/project2@2 deps/project2
[subdir]
 ^/externals/project1 deps/project1
[subdir2]
 ^/externals/project1 deps/project1
"""
        actual = repo[2]['.hgsvnexternals'].data()
        self.assertEqual(ref2, actual)

        ref3 = """[.]
 -r2 ^/externals/project2@2 deps/project2
[subdir]
 ^/externals/project1 deps/project1
"""
        self.assertEqual(ref3, repo[3]['.hgsvnexternals'].data())

        ref4 = """[subdir]
 ^/externals/project1 deps/project1
"""
        self.assertEqual(ref4, repo[4]['.hgsvnexternals'].data())

        ref5 = """[.]
 -r2 ^/externals/project2@2 deps/project2
[subdir2]
 ^/externals/project1 deps/project1
"""
        self.assertEqual(ref5, repo[5]['.hgsvnexternals'].data())

        ref6 = """[.]
 -r2 ^/externals/project2@2 deps/project2
"""
        self.assertEqual(ref6, repo[6]['.hgsvnexternals'].data())

    def test_externals_stupid(self):
        self.test_externals(True)

    def test_updateexternals(self):
        def checkdeps(deps, nodeps, repo, rev=None):
            svnexternals.updateexternals(ui, [rev], repo)
            for d in deps:
                p = os.path.join(repo.root, d)
                self.assertTrue(os.path.isdir(p),
                                'missing: %s@%r' % (d, rev))
            for d in nodeps:
                p = os.path.join(repo.root, d)
                self.assertTrue(not os.path.isdir(p),
                                'unexpected: %s@%r' % (d, rev))

        ui = self.ui()
        repo = self._load_fixture_and_fetch('externals.svndump', stupid=0)
        commands.update(ui, repo)
        checkdeps(['deps/project1'], [], repo, 0)
        checkdeps(['deps/project1', 'deps/project2'], [], repo, 1)
        checkdeps(['subdir/deps/project1', 'subdir2/deps/project1',
                   'deps/project2'],
                  ['deps/project1'], repo, 2)
        checkdeps(['subdir/deps/project1', 'deps/project2'],
                  ['subdir2/deps/project1'], repo, 3)
        checkdeps(['subdir/deps/project1'], ['deps/project2'], repo, 4)

class TestPushExternals(test_util.TestBase):
    def setUp(self):
        test_util.TestBase.setUp(self)
        test_util.load_fixture_and_fetch('pushexternals.svndump',
                                         self.repo_path,
                                         self.wc_path)

    def test_push_externals(self, stupid=False):
        # Add a new reference on an existing and non-existing directory
        changes = [
            ('.hgsvnexternals', '.hgsvnexternals',
             """[dir]
 ../externals/project2 deps/project2
[subdir1]
 ../externals/project1 deps/project1
[subdir2]
 ../externals/project2 deps/project2
"""),
            ('subdir1/a', 'subdir1/a', 'a'),
            ('subdir2/a', 'subdir2/a', 'a'),
            ]
        self.commitchanges(changes)
        self.pushrevisions(stupid)
        self.assertchanges(changes, self.repo['tip'])

        # Remove all references from one directory, add a new one
        # to the other (test multiline entries)
        changes = [
            ('.hgsvnexternals', '.hgsvnexternals',
             """[subdir1]
 ../externals/project1 deps/project1
 ../externals/project2 deps/project2
"""),
            # This removal used to trigger the parent directory removal
            ('subdir1/a', None, None),
            ]
        self.commitchanges(changes)
        self.pushrevisions(stupid)
        self.assertchanges(changes, self.repo['tip'])
        # Check subdir2/a is still there even if the externals were removed
        self.assertTrue('subdir2/a' in self.repo['tip'])
        self.assertTrue('subdir1/a' not in self.repo['tip'])

        # Test externals removal
        changes = [
            ('.hgsvnexternals', None, None),
            ]
        self.commitchanges(changes)
        self.pushrevisions(stupid)
        self.assertchanges(changes, self.repo['tip'])

    def test_push_externals_stupid(self):
        self.test_push_externals(True)


def suite():
    all = [unittest.TestLoader().loadTestsFromTestCase(TestFetchExternals),
           unittest.TestLoader().loadTestsFromTestCase(TestPushExternals),
          ]
    return unittest.TestSuite(all)
