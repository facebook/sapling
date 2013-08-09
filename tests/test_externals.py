import test_util

import os, unittest, sys

from mercurial import commands
from mercurial import util as hgutil
try:
    from mercurial import subrepo
    # require svnsubrepo and hg >= 1.7.1
    subrepo.svnsubrepo
    hgutil.checknlink
except (ImportError, AttributeError), e:
    print >> sys.stderr, 'test_externals: skipping .hgsub tests'
    subrepo = None

from hgsubversion import svnexternals

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
             ('third-party/sounds', None, 'http://svn.example.com/repos/sounds', None,
              'third-party/sounds             http://svn.example.com/repos/sounds')),

            ('third-party/skins -r148        http://svn.example.com/skinproj',
             ('third-party/skins', '148', 'http://svn.example.com/skinproj', None,
              'third-party/skins -r{REV}        http://svn.example.com/skinproj')),

            ('third-party/skins -r 148        http://svn.example.com/skinproj',
             ('third-party/skins', '148', 'http://svn.example.com/skinproj', None,
              'third-party/skins -r {REV}        http://svn.example.com/skinproj')),

            ('http://svn.example.com/repos/sounds third-party/sounds',
             ('third-party/sounds', None, 'http://svn.example.com/repos/sounds', None,
              'http://svn.example.com/repos/sounds third-party/sounds')),

            ('-r148 http://svn.example.com/skinproj third-party/skins',
             ('third-party/skins', '148', 'http://svn.example.com/skinproj', None,
              '-r{REV} http://svn.example.com/skinproj third-party/skins')),

            ('-r 148 http://svn.example.com/skinproj third-party/skins',
             ('third-party/skins', '148', 'http://svn.example.com/skinproj', None,
              '-r {REV} http://svn.example.com/skinproj third-party/skins')),

            ('http://svn.example.com/skin-maker@21 third-party/skins/toolkit',
             ('third-party/skins/toolkit', None, 'http://svn.example.com/skin-maker', '21',
              'http://svn.example.com/skin-maker@21 third-party/skins/toolkit')),
            ]

        for line, expected in samples:
            self.assertEqual(expected, svnexternals.parsedefinition(line))

    def test_externals(self, stupid=False):
        repo = self._load_fixture_and_fetch('externals.svndump', stupid=stupid)

        ref0 = """[.]
 ^/externals/project1 deps/project1
"""
        self.assertMultiLineEqual(ref0, repo[0]['.hgsvnexternals'].data())
        ref1 = """\
[.]
 # A comment, then an empty line, then a blank line
 
 ^/externals/project1 deps/project1
     
 -r2 ^/externals/project2@2 deps/project2
"""
        self.assertMultiLineEqual(ref1, repo[1]['.hgsvnexternals'].data())

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

    def test_hgsub(self, stupid=False):
        if subrepo is None:
            return
        repo = self._load_fixture_and_fetch('externals.svndump',
                                            externals='subrepos',
                                            stupid=stupid)
        self.assertEqual("""\
deps/project1 = [hgsubversion] :^/externals/project1 deps/project1
""", repo[0]['.hgsub'].data())
        self.assertEqual("""\
HEAD deps/project1
""", repo[0]['.hgsubstate'].data())

        self.assertEqual("""\
deps/project1 = [hgsubversion] :^/externals/project1 deps/project1
deps/project2 = [hgsubversion] :-r{REV} ^/externals/project2@2 deps/project2
""", repo[1]['.hgsub'].data())
        self.assertEqual("""\
HEAD deps/project1
2 deps/project2
""", repo[1]['.hgsubstate'].data())

        self.assertEqual("""\
deps/project2 = [hgsubversion] :-r{REV} ^/externals/project2@2 deps/project2
subdir/deps/project1 = [hgsubversion] subdir:^/externals/project1 deps/project1
subdir2/deps/project1 = [hgsubversion] subdir2:^/externals/project1 deps/project1
""", repo[2]['.hgsub'].data())
        self.assertEqual("""\
2 deps/project2
HEAD subdir/deps/project1
HEAD subdir2/deps/project1
""", repo[2]['.hgsubstate'].data())

        self.assertMultiLineEqual("""\
deps/project2 = [hgsubversion] :-r{REV} ^/externals/project2@2 deps/project2
subdir/deps/project1 = [hgsubversion] subdir:^/externals/project1 deps/project1
""", repo[3]['.hgsub'].data())
        self.assertEqual("""\
2 deps/project2
HEAD subdir/deps/project1
""", repo[3]['.hgsubstate'].data())

        self.assertEqual("""\
subdir/deps/project1 = [hgsubversion] subdir:^/externals/project1 deps/project1
""", repo[4]['.hgsub'].data())
        self.assertEqual("""\
HEAD subdir/deps/project1
""", repo[4]['.hgsubstate'].data())

        self.assertEqual("""\
deps/project2 = [hgsubversion] :-r{REV} ^/externals/project2@2 deps/project2
subdir2/deps/project1 = [hgsubversion] subdir2:^/externals/project1 deps/project1
""", repo[5]['.hgsub'].data())
        self.assertEqual("""\
2 deps/project2
HEAD subdir2/deps/project1
""", repo[5]['.hgsubstate'].data())

        self.assertEqual("""\
deps/project2 = [hgsubversion] :-r{REV} ^/externals/project2@2 deps/project2
""", repo[6]['.hgsub'].data())
        self.assertEqual("""\
2 deps/project2
""", repo[6]['.hgsubstate'].data())

    def test_hgsub_stupid(self):
        self.test_hgsub(True)

    def test_ignore(self):
        repo = self._load_fixture_and_fetch('externals.svndump',
                                            externals='ignore')
        for rev in repo:
            ctx = repo[rev]
            self.assertTrue('.hgsvnexternals' not in ctx)
            self.assertTrue('.hgsub' not in ctx)
            self.assertTrue('.hgsubstate' not in ctx)

    def test_updatehgsub(self):
        def checkdeps(ui, repo, rev, deps, nodeps):
            commands.update(ui, repo, node=str(rev))
            for d in deps:
                p = os.path.join(repo.root, d)
                self.assertTrue(os.path.isdir(p),
                                'missing: %s@%r' % (d, repo[None].rev()))
            for d in nodeps:
                p = os.path.join(repo.root, d)
                self.assertTrue(not os.path.isdir(p),
                                'unexpected: %s@%r' % (d, repo[None].rev()))

        if subrepo is None:
            return

        ui = self.ui()
        repo = self._load_fixture_and_fetch('externals.svndump',
                                            stupid=0, externals='subrepos')
        checkdeps(ui, repo, 0, ['deps/project1'], [])
        checkdeps(ui, repo, 1, ['deps/project1', 'deps/project2'], [])
        checkdeps(ui, repo, 2, ['subdir/deps/project1', 'subdir2/deps/project1',
                   'deps/project2'],
                  ['deps/project1'])
        checkdeps(ui, repo, 3, ['subdir/deps/project1', 'deps/project2'],
                  ['subdir2/deps/project1'])
        checkdeps(ui, repo, 4, ['subdir/deps/project1'], ['deps/project2'])

        # Test update --clean, used to crash
        repo.wwrite('subdir/deps/project1/a', 'foobar', '')
        commands.update(ui, repo, node='4', clean=True)

    def test_mergeexternals(self, stupid=False):
        if subrepo is None:
            return
        repo = self._load_fixture_and_fetch('mergeexternals.svndump',
                                            externals='subrepos',
                                            stupid=stupid)
        # Check merged directories externals are fine
        self.assertEqual("""\
d1/ext = [hgsubversion] d1:^/trunk/common/ext ext
d2/ext = [hgsubversion] d2:^/trunk/common/ext ext
d3/ext3 = [hgsubversion] d3:^/trunk/common/ext ext3
""", repo['tip']['.hgsub'].data())

    def test_mergeexternals_stupid(self):
        self.test_mergeexternals(True)

class TestPushExternals(test_util.TestBase):
    obsolete_mode_tests = True

    def test_push_externals(self, stupid=False):
        repo = self._load_fixture_and_fetch('pushexternals.svndump')
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

    def test_push_hgsub(self, stupid=False):
        if subrepo is None:
            return

        repo, repo_path = self.load_and_fetch('pushexternals.svndump',
                                              externals='subrepos')
        # Add a new reference on an existing and non-existing directory
        changes = [
            ('.hgsub', '.hgsub', """\
dir/deps/project2 = [hgsubversion] dir:^/externals/project2 deps/project2
subdir1/deps/project1 = [hgsubversion] subdir1:^/externals/project1 deps/project1
subdir2/deps/project2 = [hgsubversion] subdir2:^/externals/project2 deps/project2
"""),
            ('.hgsubstate', '.hgsubstate', """\
HEAD dir/deps/project2
HEAD subdir1/deps/project1
HEAD subdir2/deps/project2
"""),
            ('subdir1/a', 'subdir1/a', 'a'),
            ('subdir2/a', 'subdir2/a', 'a'),
            ]
        self.svnco(repo_path, 'externals/project2', '2', 'dir/deps/project2')
        self.svnco(repo_path, 'externals/project1', '2', 'subdir1/deps/project1')
        self.svnco(repo_path, 'externals/project2', '2', 'subdir2/deps/project2')
        self.commitchanges(changes)
        self.pushrevisions(stupid)
        self.assertchanges(changes, self.repo['tip'])

        # Check .hgsub and .hgsubstate were not pushed
        self.assertEqual(['dir', 'subdir1', 'subdir1/a', 'subdir2',
                          'subdir2/a'], test_util.svnls(repo_path, 'trunk'))

        # Remove all references from one directory, add a new one
        # to the other (test multiline entries)
        changes = [
            ('.hgsub', '.hgsub', """\
subdir1/deps/project1 = [hgsubversion] subdir1:^/externals/project1 deps/project1
subdir1/deps/project2 = [hgsubversion] subdir1:^/externals/project2 deps/project2
"""),
            ('.hgsubstate', '.hgsubstate', """\
HEAD subdir1/deps/project1
HEAD subdir1/deps/project2
"""),
            # This removal used to trigger the parent directory removal
            ('subdir1/a', None, None),
            ]
        self.svnco(repo_path, 'externals/project1', '2', 'subdir1/deps/project1')
        self.svnco(repo_path, 'externals/project2', '2', 'subdir1/deps/project2')
        self.commitchanges(changes)
        self.pushrevisions(stupid)
        self.assertchanges(changes, self.repo['tip'])
        # Check subdir2/a is still there even if the externals were removed
        self.assertTrue('subdir2/a' in self.repo['tip'])
        self.assertTrue('subdir1/a' not in self.repo['tip'])

        # Move the externals so they are defined on the base directory,
        # this used to cause full branch removal when deleting the .hgsub
        changes = [
            ('.hgsub', '.hgsub', """\
subdir1/deps/project1 = [hgsubversion] :^/externals/project1 subdir1/deps/project1
"""),
            ('.hgsubstate', '.hgsubstate', """\
HEAD subdir1/deps/project1
"""),
            ]
        self.commitchanges(changes)
        self.pushrevisions(stupid)
        self.assertchanges(changes, self.repo['tip'])

        # Test externals removal
        changes = [
            ('.hgsub', None, None),
            ('.hgsubstate', None, None),
            ]
        self.commitchanges(changes)
        self.pushrevisions(stupid)
        self.assertchanges(changes, self.repo['tip'])
