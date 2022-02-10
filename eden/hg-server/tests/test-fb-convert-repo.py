import os
import unittest

from edenscm.hgext.convert.repo import conversionrevision, gitutil, repo, repomanifest
from hghave import require


def draft(test_func):
    if os.environ["USER"] not in ("mdevine", "tch"):

        def skip_func(self):
            return unittest.skip("Skipping draft test %s" % test_func.__name__)

        return skip_func
    return test_func


class gitutiltest(unittest.TestCase):
    """Unit tests for the gitutil helper class of the repo converter"""

    def test_getfilemodestr(self):
        mode = gitutil.getfilemodestr(int("000644", 8))
        self.assertEqual("", mode)

        mode = gitutil.getfilemodestr(int("100755", 8))
        self.assertEqual("x", mode)

        mode = gitutil.getfilemodestr(int("120000", 8))
        self.assertEqual("l", mode)

    def test_parsedifftree_readable(self):
        difftree_string = """1fd915b9c1fcf3803383432ede29fc4d686fdb44
:100644 100644 b02a70992e734985768e839281932c315fafb21d a268f9d1a620a9f438d014376f72bcf413eea6d8 M\tlibc/arch-arm/bionic/__bionic_clone.S
:100644 100644 56ac0f69d450d218174226e1d61863a1ce5d4f27 27e44e7f7598ee1d2ca13df305f269d8ce303bfb M\tlibc/arch-arm64/bionic/__bionic_clone.S
1fd915b9c1fcf3803383432ede29fc4d686fdb44
:100644 100644 6439e31ce8bd4e4f1e290c3d8607cb7694a52b31 a8da3ac753c77d4a26f95bffcca546cc8cfb4f77 M\tlibc/dns/resolv/res_send.c
"""
        out = gitutil.parsedifftree(difftree_string)
        expected = [
            [
                {
                    "source": {
                        "mode": 0o100644,
                        "hash": "b02a70992e734985768e839281932c315fafb21d",
                        "path": "libc/arch-arm/bionic/__bionic_clone.S",
                    },
                    "dest": {
                        "mode": 0o100644,
                        "hash": "a268f9d1a620a9f438d014376f72bcf413eea6d8",
                        "path": "libc/arch-arm/bionic/__bionic_clone.S",
                    },
                    "status": "M",
                    "score": None,
                },
                {
                    "source": {
                        "mode": 0o100644,
                        "hash": "56ac0f69d450d218174226e1d61863a1ce5d4f27",
                        "path": "libc/arch-arm64/bionic/__bionic_clone.S",
                    },
                    "dest": {
                        "mode": 0o100644,
                        "hash": "27e44e7f7598ee1d2ca13df305f269d8ce303bfb",
                        "path": "libc/arch-arm64/bionic/__bionic_clone.S",
                    },
                    "status": "M",
                    "score": None,
                },
            ],
            [
                {
                    "source": {
                        "mode": 0o100644,
                        "hash": "6439e31ce8bd4e4f1e290c3d8607cb7694a52b31",
                        "path": "libc/dns/resolv/res_send.c",
                    },
                    "dest": {
                        "mode": 0o100644,
                        "hash": "a8da3ac753c77d4a26f95bffcca546cc8cfb4f77",
                        "path": "libc/dns/resolv/res_send.c",
                    },
                    "status": "M",
                    "score": None,
                }
            ],
        ]
        self.assertEqual(out, expected)

    def test_parsedifftree_compact(self):
        difftree_string = """1fd915b9c1fcf3803383432ede29fc4d686fdb44\x00:100644 100644 b02a70992e734985768e839281932c315fafb21d a268f9d1a620a9f438d014376f72bcf413eea6d8 M\x00libc/arch-arm/bionic/__bionic_clone.S\x00:100644 100644 56ac0f69d450d218174226e1d61863a1ce5d4f27 27e44e7f7598ee1d2ca13df305f269d8ce303bfb M\x00libc/arch-arm64/bionic/__bionic_clone.S\x001fd915b9c1fcf3803383432ede29fc4d686fdb44\x00:100644 100644 6439e31ce8bd4e4f1e290c3d8607cb7694a52b31 a8da3ac753c77d4a26f95bffcca546cc8cfb4f77 M\x00libc/dns/resolv/res_send.c\x00"""
        out = gitutil.parsedifftree(difftree_string)
        expected = [
            [  # Parent 1
                {  # File 1
                    "source": {
                        "mode": 0o100644,
                        "hash": "b02a70992e734985768e839281932c315fafb21d",
                        "path": "libc/arch-arm/bionic/__bionic_clone.S",
                    },
                    "dest": {
                        "mode": 0o100644,
                        "hash": "a268f9d1a620a9f438d014376f72bcf413eea6d8",
                        "path": "libc/arch-arm/bionic/__bionic_clone.S",
                    },
                    "status": "M",
                    "score": None,
                },
                {  # File 2
                    "source": {
                        "mode": 0o100644,
                        "hash": "56ac0f69d450d218174226e1d61863a1ce5d4f27",
                        "path": "libc/arch-arm64/bionic/__bionic_clone.S",
                    },
                    "dest": {
                        "mode": 0o100644,
                        "hash": "27e44e7f7598ee1d2ca13df305f269d8ce303bfb",
                        "path": "libc/arch-arm64/bionic/__bionic_clone.S",
                    },
                    "status": "M",
                    "score": None,
                },
            ],
            [  # Parent 2
                {  # File 1
                    "source": {
                        "mode": 0o100644,
                        "hash": "6439e31ce8bd4e4f1e290c3d8607cb7694a52b31",
                        "path": "libc/dns/resolv/res_send.c",
                    },
                    "dest": {
                        "mode": 0o100644,
                        "hash": "a8da3ac753c77d4a26f95bffcca546cc8cfb4f77",
                        "path": "libc/dns/resolv/res_send.c",
                    },
                    "status": "M",
                    "score": None,
                }
            ],
        ]
        self.assertEqual(out, expected)

    def test_parsegitcommitraw(self):
        commit_hash = "6c6677a7b5cf683a1883bc5e4ad47cad0a496904"
        commit_string = """tree e2acedaa094c4b5f0606e2a5ff58c3648555cfd4
parent c6c89b3401f3f6690e2307de7e2d079894c8147a
parent 2051d0428d045796ded3764c4188249669d1fcf3
author Linux Build Service Account <lnxbuild@localhost> 1521780995 -0700
committer Linux Build Service Account <lnxbuild@localhost> 1521780995 -0700

Merge AU_LINUX_ANDROID_LA.BR.1.3.7_RB1.08.01.00.336.038 on remote branch

Change-Id: Ie8ded3a8316b465c89a256c1a9146345614ed68f"""
        out = gitutil.parsegitcommitraw(commit_hash, commit_string)

        self.assertEqual(out.rev, "6c6677a7b5cf683a1883bc5e4ad47cad0a496904")
        self.assertSequenceEqual(
            out.parents,
            [
                "c6c89b3401f3f6690e2307de7e2d079894c8147a",
                "2051d0428d045796ded3764c4188249669d1fcf3",
            ],
        )
        self.assertEqual(
            out.desc,
            """Merge AU_LINUX_ANDROID_LA.BR.1.3.7_RB1.08.01.00.336.038 on remote branch

Change-Id: Ie8ded3a8316b465c89a256c1a9146345614ed68f""",
        )


class repotest(unittest.TestCase):
    """Tests implementation of the repo command"""

    def test_forallbyproject(self):
        foralloutput = """project A/
123
456
789
0

project B/
Humpty Dumpty
sat on the wall
Humpty Dumpty
had a great fall
"""
        out = repo._splitlinesbyproject(foralloutput)
        self.assertSequenceEqual(out["A/"], ["123", "456", "789", "0"])
        self.assertSequenceEqual(
            out["B/"],
            ["Humpty Dumpty", "sat on the wall", "Humpty Dumpty", "had a great fall"],
        )


class conversionrevisiontest(unittest.TestCase):
    """Tests implementation of the conversionrevision class"""

    def test_init(self) -> None:
        _ = conversionrevision(
            conversionrevision.VARIANT_UNIFIED,
            "1234567890123456789012345678901234567890",
            "aosp/bzip2",
            "external/bzip2",
        )
        self.assertTrue(True)

    def test_variant(self) -> None:
        rev = conversionrevision(
            conversionrevision.VARIANT_ROOTED,
            "1234567890123456789012345678901234567890",
            "aosp/bzip2",
            "external/bzip2",
        )
        self.assertEqual(rev.variant, conversionrevision.VARIANT_ROOTED)

    def test_sourcehash(self) -> None:
        rev = conversionrevision(
            conversionrevision.VARIANT_UNIFIED,
            "1234567890123456789012345678901234567890",
            "aosp/bzip2",
            "external/bzip2",
        )
        self.assertEqual(rev.sourcehash, "1234567890123456789012345678901234567890")

    def test_sourceproject(self) -> None:
        rev = conversionrevision(
            conversionrevision.VARIANT_UNIFIED,
            "1234567890123456789012345678901234567890",
            "aosp/bzip2",
            "external/bzip2",
        )
        self.assertEqual(rev.sourceproject, "aosp/bzip2")

    def test_destpath(self) -> None:
        rev = conversionrevision(
            conversionrevision.VARIANT_UNIFIED,
            "1234567890123456789012345678901234567890",
            "aosp/bzip2",
            "external/bzip2",
        )
        self.assertEqual(rev.destpath, "external/bzip2")

    def test_parse(self) -> None:
        revstring = "R1234567890123456789012345678901234567890foo/bar/baz:external/baz"
        rev = conversionrevision.parse(revstring)
        self.assertEqual(rev.variant, conversionrevision.VARIANT_ROOTED)
        self.assertEqual(rev.sourcehash, "1234567890123456789012345678901234567890")
        self.assertEqual(rev.sourceproject, "foo/bar/baz")
        self.assertEqual(rev.destpath, "external/baz")

    def test_str(self) -> None:
        rev = conversionrevision(
            conversionrevision.VARIANT_UNIFIED,
            "1234567890123456789012345678901234567890",
            "aosp/bzip2",
            "external/bzip2",
        )
        revstring = "U1234567890123456789012345678901234567890aosp/bzip2:external/bzip2"
        self.assertEqual(str(rev), revstring)

    def test_equals(self) -> None:
        rev1 = conversionrevision(
            conversionrevision.VARIANT_UNIFIED,
            "1234567890123456789012345678901234567890",
            "aosp/bzip2",
            "external/bzip2",
        )
        rev2 = conversionrevision(
            conversionrevision.VARIANT_UNIFIED,
            "1234567890123456789012345678901234567890",
            "aosp/bzip2",
            "external/bzip2",
        )
        rev3 = conversionrevision(
            conversionrevision.VARIANT_ROOTED,
            "1234567890123456789012345678901234567890",
            "aosp/bzip2",
            "external/bzip2",
        )

        self.assertTrue(rev1 == rev2)
        self.assertFalse(rev1 == rev3)
        self.assertFalse(rev1 == conversionrevision.NONE)

    def test_hash(self) -> None:
        rev = conversionrevision(
            conversionrevision.VARIANT_UNIFIED,
            "1234567890123456789012345678901234567890",
            "aosp/bzip2",
            "external/bzip2",
        )
        out = hash(rev)
        self.assertIsNotNone(out)
        self.assertIsInstance(hash(rev), int)


class repomanifesttest(unittest.TestCase):
    """Tests implementation of the repomanifest class"""

    def test_fromtext(self):
        manifestblobs = {
            "default.xml": """<?xml version="1.0" encoding="UTF-8"?>
<manifest>
  <remote
    name="origin"
    fetch="ssh://git-ro.vip.facebook.com/data/gitrepos"
    push="ssh://git.vip.facebook.com/data/gitrepos"
    pushurl="ssh://git.vip.facebook.com/data/gitrepos"/>

  <default
    remote="origin"
    revision="mydefaultbranch"/>

  <project name="foo/alpha" path="A"/>
  <project name="oculus/foo/bravo" path="vendor/b">
    <linkfile dest=".watchmanconfig" src="watchmanconfig"/>
    <annotation name="not_old" value="37"/>
  </project>
</manifest>
"""
        }
        manifest = repomanifest.fromtext("default.xml", manifestblobs)
        self.assertIsNotNone(manifest)

    def test_getprojectrevision(self):
        manifestblobs = {
            "manifest.xml": """<?xml version="1.0" encoding="UTF-8"?>
<manifest>
  <remote
    name="origin"
    fetch="ssh://git-ro.vip.facebook.com/data/gitrepos"
    push="ssh://git.vip.facebook.com/data/gitrepos"
    pushurl="ssh://git.vip.facebook.com/data/gitrepos"/>

  <default
    remote="origin"
    revision="mydefaultbranch"/>

  <project name="foo/alpha" path="A"/>
  <project name="oculus/foo/bravo" path="vendor/b" revision="aosp-tb12">
    <linkfile dest=".watchmanconfig" src="watchmanconfig"/>
    <annotation name="not_old" value="37"/>
  </project>
</manifest>
"""
        }
        manifest = repomanifest.fromtext("manifest.xml", manifestblobs)
        self.assertEqual(
            manifest.getprojectrevision("foo/alpha"), "origin/mydefaultbranch"
        )
        self.assertEqual(
            manifest.getprojectrevision("oculus/foo/bravo"), "origin/aosp-tb12"
        )

    def test_getprojectpaths(self):
        manifestblobs = {
            "blob1": """<?xml version="1.0" encoding="UTF-8"?>
<manifest>
  <remote
    name="origin"
    fetch="ssh://git-ro.vip.facebook.com/data/gitrepos"
    push="ssh://git.vip.facebook.com/data/gitrepos"
    pushurl="ssh://git.vip.facebook.com/data/gitrepos"/>

  <default
    remote="origin"
    revision="mydefaultbranch"/>

  <project name="foo/alpha" path="A"/>
  <project name="oculus/foo/bravo" path="vendor/b/monterey" revision="monterey" />
  <project name="oculus/foo/bravo" path="vendor/b/pacific" revision="pacific" />
</manifest>
"""
        }
        manifest = repomanifest.fromtext("blob1", manifestblobs)
        self.assertEqual(manifest.getprojectpaths("foo/alpha"), ["A"])
        self.assertEqual(
            manifest.getprojectpaths("oculus/foo/bravo"),
            ["vendor/b/monterey", "vendor/b/pacific"],
        )

    def test_getprojectpathrevisions(self):
        manifestblobs = {
            "default.xml": """<?xml version="1.0" encoding="UTF-8"?>
<manifest>
  <remote
    name="origin"
    fetch="ssh://git-ro.vip.facebook.com/data/gitrepos"
    push="ssh://git.vip.facebook.com/data/gitrepos"
    pushurl="ssh://git.vip.facebook.com/data/gitrepos"/>

  <remote name="myremote"/>
  <default
    remote="myremote"
    revision="mydefaultbranch"/>

  <project name="foo/alpha" path="A" remote="origin"/>
  <project name="oculus/foo/bravo" path="vendor/b/monterey" revision="monterey" />
  <project name="oculus/foo/bravo" path="vendor/b/pacific" revision="pacific" />
</manifest>
"""
        }
        manifest = repomanifest.fromtext("default.xml", manifestblobs)
        self.assertEqual(
            manifest.getprojectpathrevisions("foo/alpha"),
            {"A": "origin/mydefaultbranch"},
        )
        self.assertEqual(
            manifest.getprojectpathrevisions("oculus/foo/bravo"),
            {
                "vendor/b/monterey": "myremote/monterey",
                "vendor/b/pacific": "myremote/pacific",
            },
        )

    def test_getprojects(self):
        manifestblobs = {
            "default.xml": """<?xml version="1.0" encoding="UTF-8"?>
<manifest>
  <remote
    name="origin"
    fetch="ssh://git-ro.vip.facebook.com/data/gitrepos"
    push="ssh://git.vip.facebook.com/data/gitrepos"
    pushurl="ssh://git.vip.facebook.com/data/gitrepos"/>

  <default
    remote="origin"
    revision="mydefaultbranch"/>

  <project name="foo/alpha" path="A"/>
  <project name="oculus/foo/bravo" path="vendor/b/monterey" revision="monterey" />
  <project name="oculus/foo/bravo" path="vendor/b/pacific" revision="pacific" />
</manifest>
"""
        }
        manifest = repomanifest.fromtext("default.xml", manifestblobs)
        expected = [
            ("foo/alpha", "A", "origin/mydefaultbranch"),
            ("oculus/foo/bravo", "vendor/b/monterey", "origin/monterey"),
            ("oculus/foo/bravo", "vendor/b/pacific", "origin/pacific"),
        ]
        self.assertEqual(manifest.getprojects(), expected)

    def test_getprojects_hashes(self):
        manifestblobs = {
            "default.xml": """<?xml version="1.0" encoding="UTF-8"?>
<manifest>
  <remote
    name="origin"
    fetch="ssh://git-ro.vip.facebook.com/data/gitrepos"
    push="ssh://git.vip.facebook.com/data/gitrepos"
    pushurl="ssh://git.vip.facebook.com/data/gitrepos"/>

  <default
    remote="origin"
    revision="mydefaultbranch"/>

  <project name="foo/alpha" path="A"/>
  <project name="oculus/foo/bravo" path="vendor/b/monterey" revision="monterey" />
  <project name="oculus/foo/bravo" path="vendor/b/pacific" revision="5fa2a4dbfb5616ffd2d32702f6f97be331e665a6" />
</manifest>
"""
        }
        manifest = repomanifest.fromtext("default.xml", manifestblobs)
        expected = [
            ("foo/alpha", "A", "origin/mydefaultbranch"),
            ("oculus/foo/bravo", "vendor/b/monterey", "origin/monterey"),
            (
                "oculus/foo/bravo",
                "vendor/b/pacific",
                "5fa2a4dbfb5616ffd2d32702f6f97be331e665a6",
            ),
        ]
        self.assertEqual(manifest.getprojects(), expected)

    def test_include(self):
        manifestblobs = {
            "default.xml": """<?xml version="1.0" encoding="UTF-8"?>
<manifest>
  <remote
    name="origin"
    fetch="ssh://git-ro.vip.facebook.com/data/gitrepos"
    push="ssh://git.vip.facebook.com/data/gitrepos"
    pushurl="ssh://git.vip.facebook.com/data/gitrepos"/>

  <default
    remote="origin"
    revision="mydefaultbranch"/>

  <include name="include-example.xml"/>
  <project name="foo/alpha" path="A"/>
  <project name="oculus/foo/bravo" path="vendor/b/monterey" revision="monterey" />
  <project name="oculus/foo/bravo" path="vendor/b/pacific" revision="pacific" />
</manifest>
""",
            "include-example.xml": """<?xml version="1.0" encoding="UTF-8"?>
<manifest>
  <project name="include-c" path="include/c" />
</manifest>
""",
        }
        manifest = repomanifest.fromtext("default.xml", manifestblobs)
        self.assertTrue(manifest.hasproject("include-c"))
        self.assertEqual(manifest.getprojectpaths("include-c"), ["include/c"])
        self.assertEqual(
            manifest.getprojectrevision("include-c"), "origin/mydefaultbranch"
        )


if __name__ == "__main__":
    import silenttestrunner

    silenttestrunner.main(__name__)
