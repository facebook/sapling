# no-check-code -- see T24862348

from __future__ import absolute_import

import os

import test_hgsubversion_util
from edenscm.hgext.hgsubversion import svnexternals
from edenscm.mercurial import commands


class TestFetchExternals(test_hgsubversion_util.TestBase):
    stupid_mode_tests = True

    def test_externalsfile(self):
        f = svnexternals.externalsfile()
        f["t1"] = "dir1 -r10 svn://foobar"
        f["t 2"] = "dir2 -r10 svn://foobar"
        f["t3"] = ["dir31 -r10 svn://foobar", "dir32 -r10 svn://foobar"]

        refext = (
            """[t 2]\n"""
            """ dir2 -r10 svn://foobar\n"""
            """[t1]\n"""
            """ dir1 -r10 svn://foobar\n"""
            """[t3]\n"""
            """ dir31 -r10 svn://foobar\n"""
            """ dir32 -r10 svn://foobar\n"""
        )
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
            (
                "third-party/sounds             http://svn.example.com/repos/sounds",
                (
                    "third-party/sounds",
                    None,
                    "http://svn.example.com/repos/sounds",
                    None,
                    "third-party/sounds             http://svn.example.com/repos/sounds",
                ),
            ),
            (
                "third-party/skins -r148        http://svn.example.com/skinproj",
                (
                    "third-party/skins",
                    "148",
                    "http://svn.example.com/skinproj",
                    None,
                    "third-party/skins -r{REV}        http://svn.example.com/skinproj",
                ),
            ),
            (
                "third-party/skins -r 148        http://svn.example.com/skinproj",
                (
                    "third-party/skins",
                    "148",
                    "http://svn.example.com/skinproj",
                    None,
                    "third-party/skins -r {REV}        http://svn.example.com/skinproj",
                ),
            ),
            (
                "http://svn.example.com/repos/sounds third-party/sounds",
                (
                    "third-party/sounds",
                    None,
                    "http://svn.example.com/repos/sounds",
                    None,
                    "http://svn.example.com/repos/sounds third-party/sounds",
                ),
            ),
            (
                "-r148 http://svn.example.com/skinproj third-party/skins",
                (
                    "third-party/skins",
                    "148",
                    "http://svn.example.com/skinproj",
                    None,
                    "-r{REV} http://svn.example.com/skinproj third-party/skins",
                ),
            ),
            (
                "-r 148 http://svn.example.com/skinproj third-party/skins",
                (
                    "third-party/skins",
                    "148",
                    "http://svn.example.com/skinproj",
                    None,
                    "-r {REV} http://svn.example.com/skinproj third-party/skins",
                ),
            ),
            (
                "http://svn.example.com/skin-maker@21 third-party/skins/toolkit",
                (
                    "third-party/skins/toolkit",
                    None,
                    "http://svn.example.com/skin-maker",
                    "21",
                    "http://svn.example.com/skin-maker@21 third-party/skins/toolkit",
                ),
            ),
        ]

        for line, expected in samples:
            self.assertEqual(expected, svnexternals.parsedefinition(line))

    def test_externals(self):
        repo = self._load_fixture_and_fetch("externals.svndump")

        ref0 = """[.]
 ^/externals/project1 deps/project1
"""
        self.assertMultiLineEqual(ref0, repo[0][".hgsvnexternals"].data())
        ref1 = (
            """[.]\n"""
            """ # A comment, then an empty line, then a blank line\n"""
            """ \n"""
            """ ^/externals/project1 deps/project1\n"""
            """     \n"""
            """ -r2 ^/externals/project2@2 deps/project2\n"""
        )
        self.assertMultiLineEqual(ref1, repo[1][".hgsvnexternals"].data())

        ref2 = (
            """[.]\n"""
            """ -r2 ^/externals/project2@2 deps/project2\n"""
            """[subdir]\n"""
            """ ^/externals/project1 deps/project1\n"""
            """[subdir2]\n"""
            """ ^/externals/project1 deps/project1\n"""
        )
        actual = repo[2][".hgsvnexternals"].data()
        self.assertEqual(ref2, actual)

        ref3 = (
            """[.]\n"""
            """ -r2 ^/externals/project2@2 deps/project2\n"""
            """[subdir]\n"""
            """ ^/externals/project1 deps/project1\n"""
        )
        self.assertEqual(ref3, repo[3][".hgsvnexternals"].data())

        ref4 = """[subdir]\n""" """ ^/externals/project1 deps/project1\n"""
        self.assertEqual(ref4, repo[4][".hgsvnexternals"].data())

        ref5 = (
            """[.]\n"""
            """ -r2 ^/externals/project2@2 deps/project2\n"""
            """[subdir2]\n"""
            """ ^/externals/project1 deps/project1\n"""
        )
        self.assertEqual(ref5, repo[5][".hgsvnexternals"].data())

        ref6 = """[.]\n""" """ -r2 ^/externals/project2@2 deps/project2\n"""
        self.assertEqual(ref6, repo[6][".hgsvnexternals"].data())

    def test_updateexternals(self):
        def checkdeps(deps, nodeps, repo, rev=None):
            svnexternals.updateexternals(ui, [rev], repo)
            for d in deps:
                p = os.path.join(repo.root, d)
                self.assertTrue(os.path.isdir(p), "missing: %s@%r" % (d, rev))
            for d in nodeps:
                p = os.path.join(repo.root, d)
                self.assertTrue(not os.path.isdir(p), "unexpected: %s@%r" % (d, rev))

        ui = self.ui()
        repo = self._load_fixture_and_fetch("externals.svndump")
        commands.update(ui, repo)
        checkdeps(["deps/project1"], [], repo, 0)
        checkdeps(["deps/project1", "deps/project2"], [], repo, 1)
        checkdeps(
            ["subdir/deps/project1", "subdir2/deps/project1", "deps/project2"],
            ["deps/project1"],
            repo,
            2,
        )
        checkdeps(
            ["subdir/deps/project1", "deps/project2"],
            ["subdir2/deps/project1"],
            repo,
            3,
        )
        checkdeps(["subdir/deps/project1"], ["deps/project2"], repo, 4)

    def test_ignore(self):
        repo = self._load_fixture_and_fetch("externals.svndump", externals="ignore")
        for rev in repo:
            ctx = repo[rev]
            self.assertTrue(".hgsvnexternals" not in ctx)
            self.assertTrue(".hgsub" not in ctx)
            self.assertTrue(".hgsubstate" not in ctx)


class TestPushExternals(test_hgsubversion_util.TestBase):
    stupid_mode_tests = True
    obsolete_mode_tests = True

    def test_push_externals(self):
        self._load_fixture_and_fetch("pushexternals.svndump")
        # Add a new reference on an existing and non-existing directory
        changes = [
            (
                ".hgsvnexternals",
                ".hgsvnexternals",
                """[dir]\n"""
                """ ../externals/project2 deps/project2\n"""
                """[subdir1]\n"""
                """ ../externals/project1 deps/project1\n"""
                """[subdir2]\n"""
                """ ../externals/project2 deps/project2\n""",
            ),
            ("subdir1/a", "subdir1/a", "a"),
            ("subdir2/a", "subdir2/a", "a"),
        ]
        self.commitchanges(changes)
        self.pushrevisions()
        self.assertchanges(changes, self.repo["tip"])

        # Remove all references from one directory, add a new one
        # to the other (test multiline entries)
        changes = [
            (
                ".hgsvnexternals",
                ".hgsvnexternals",
                """[subdir1]\n"""
                """ ../externals/project1 deps/project1\n"""
                """ ../externals/project2 deps/project2\n""",
            ),
            # This removal used to trigger the parent directory removal
            ("subdir1/a", None, None),
        ]
        self.commitchanges(changes)
        self.pushrevisions()
        self.assertchanges(changes, self.repo["tip"])
        # Check subdir2/a is still there even if the externals were removed
        self.assertTrue("subdir2/a" in self.repo["tip"])
        self.assertTrue("subdir1/a" not in self.repo["tip"])

        # Test externals removal
        changes = [(".hgsvnexternals", None, None)]
        self.commitchanges(changes)
        self.pushrevisions()
        self.assertchanges(changes, self.repo["tip"])


if __name__ == "__main__":
    import silenttestrunner

    silenttestrunner.main(__name__)
