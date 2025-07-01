from __future__ import absolute_import

import binascii
import itertools
import os
import unittest

import silenttestrunner
from sapling import manifest as manifestmod, match as matchmod


EMPTY_MANIFEST = b""

HASH_1 = b"1" * 40
BIN_HASH_1 = binascii.unhexlify(HASH_1)
HASH_2 = b"f" * 40
BIN_HASH_2 = binascii.unhexlify(HASH_2)
HASH_3 = b"1234567890abcdef0987654321deadbeef0fcafe"
BIN_HASH_3 = binascii.unhexlify(HASH_3)
A_SHORT_MANIFEST = (
    b"bar/baz/qux.py\0%(hash2)s%(flag2)s\nfoo\0%(hash1)s%(flag1)s\n"
) % {b"hash1": HASH_1, b"flag1": b"", b"hash2": HASH_2, b"flag2": b"l"}

A_STEM_COMPRESSED_MANIFEST = (
    (
        b"\0\n"
        b"\x00bar/baz/qux.py\0%(flag2)s\n%(hash2)s\n"
        b"\x04qux/foo.py\0%(flag1)s\n%(hash1)s\n"  # simple case of 4 stem chars
        b"\x0az.py\0%(flag1)s\n%(hash1)s\n"  # tricky newline = 10 stem characters
        b"\x00%(verylongdir)sx/x\0\n%(hash1)s\n"
        b"\xffx/y\0\n%(hash2)s\n"  # more than 255 stem chars
    )
    % {
        b"hash1": BIN_HASH_1,
        b"flag1": b"",
        b"hash2": BIN_HASH_2,
        b"flag2": b"l",
        b"verylongdir": 255 * b"x",
    }
)

A_DEEPER_MANIFEST = (
    b"a/b/c/bar.py\0%(hash3)s%(flag1)s\n"
    b"a/b/c/bar.txt\0%(hash1)s%(flag1)s\n"
    b"a/b/c/foo.py\0%(hash3)s%(flag1)s\n"
    b"a/b/c/foo.txt\0%(hash2)s%(flag2)s\n"
    b"a/b/d/baz.py\0%(hash3)s%(flag1)s\n"
    b"a/b/d/qux.py\0%(hash1)s%(flag2)s\n"
    b"a/b/d/ten.txt\0%(hash3)s%(flag2)s\n"
    b"a/b/dog.py\0%(hash3)s%(flag1)s\n"
    b"a/b/fish.py\0%(hash2)s%(flag1)s\n"
    b"a/c/london.py\0%(hash3)s%(flag2)s\n"
    b"a/c/paper.txt\0%(hash2)s%(flag2)s\n"
    b"a/c/paris.py\0%(hash2)s%(flag1)s\n"
    b"a/d/apple.py\0%(hash3)s%(flag1)s\n"
    b"a/d/pizza.py\0%(hash3)s%(flag2)s\n"
    b"a/green.py\0%(hash1)s%(flag2)s\n"
    b"a/purple.py\0%(hash2)s%(flag1)s\n"
    b"app.py\0%(hash3)s%(flag1)s\n"
    b"readme.txt\0%(hash2)s%(flag1)s\n"
) % {
    b"hash1": HASH_1,
    b"flag1": b"",
    b"hash2": HASH_2,
    b"flag2": b"l",
    b"hash3": HASH_3,
}

HUGE_MANIFEST_ENTRIES = 200001

izip = getattr(itertools, "izip", zip)
if "xrange" not in globals():
    xrange = range

A_HUGE_MANIFEST = b"".join(
    sorted(
        b"file%d\0%s%s\n" % (i, h, f)
        for i, h, f in izip(
            xrange(200001),
            itertools.cycle((HASH_1, HASH_2)),
            itertools.cycle((b"", b"x", b"l")),
        )
    )
)


class basemanifesttests:
    def parsemanifest(self, text):
        raise NotImplementedError("parsemanifest not implemented by test case")

    def testEmptyManifest(self):
        m = self.parsemanifest(EMPTY_MANIFEST)
        self.assertEqual(0, len(m))
        self.assertEqual([], list(m))

    def testManifest(self):
        m = self.parsemanifest(A_SHORT_MANIFEST)
        self.assertEqual(["bar/baz/qux.py", "foo"], list(m))
        self.assertEqual(BIN_HASH_2, m["bar/baz/qux.py"])
        self.assertEqual("l", m.flags("bar/baz/qux.py"))
        self.assertEqual(BIN_HASH_1, m["foo"])
        self.assertEqual("", m.flags("foo"))
        with self.assertRaises(KeyError):
            m["wat"]

    def testSetItem(self):
        want = BIN_HASH_1

        m = self.parsemanifest(EMPTY_MANIFEST)
        m["a"] = want
        self.assertIn("a", m)
        self.assertEqual(want, m["a"])
        self.assertEqual(b"a\0" + HASH_1 + b"\n", m.text())

        m = self.parsemanifest(A_SHORT_MANIFEST)
        m["a"] = want
        self.assertEqual(want, m["a"])
        self.assertEqual(b"a\0" + HASH_1 + b"\n" + A_SHORT_MANIFEST, m.text())

    def testSetFlag(self):
        want = "x"
        wantb = b"x"

        m = self.parsemanifest(EMPTY_MANIFEST)
        # first add a file; a file-less flag makes no sense
        m["a"] = BIN_HASH_1
        m.setflag("a", want)
        self.assertEqual(want, m.flags("a"))
        self.assertEqual(b"a\0" + HASH_1 + wantb + b"\n", m.text())

        m = self.parsemanifest(A_SHORT_MANIFEST)
        # first add a file; a file-less flag makes no sense
        m["a"] = BIN_HASH_1
        m.setflag("a", want)
        self.assertEqual(want, m.flags("a"))
        self.assertEqual(b"a\0" + HASH_1 + wantb + b"\n" + A_SHORT_MANIFEST, m.text())

    def testCopy(self):
        m = self.parsemanifest(A_SHORT_MANIFEST)
        m["a"] = BIN_HASH_1
        m2 = m.copy()
        del m
        del m2  # make sure we don't double free() anything

    def testCompaction(self):
        unhex = binascii.unhexlify
        h1, h2 = unhex(HASH_1), unhex(HASH_2)
        m = self.parsemanifest(A_SHORT_MANIFEST)
        m["alpha"] = h1
        m["beta"] = h2
        del m["foo"]
        want = b"alpha\0%s\nbar/baz/qux.py\0%sl\nbeta\0%s\n" % (HASH_1, HASH_2, HASH_2)
        self.assertEqual(want, m.text())
        self.assertEqual(3, len(m))
        self.assertEqual(["alpha", "bar/baz/qux.py", "beta"], list(m))
        self.assertEqual(h1, m["alpha"])
        self.assertEqual(h2, m["bar/baz/qux.py"])
        self.assertEqual(h2, m["beta"])
        self.assertEqual("", m.flags("alpha"))
        self.assertEqual("l", m.flags("bar/baz/qux.py"))
        self.assertEqual("", m.flags("beta"))
        with self.assertRaises(KeyError):
            m["foo"]

    def testSetGetNodeSuffix(self):
        clean = self.parsemanifest(A_SHORT_MANIFEST)
        m = self.parsemanifest(A_SHORT_MANIFEST)
        h = m["foo"]
        f = m.flags("foo")
        want = h + b"a"
        # Merge code wants to set 21-byte fake hashes at times
        m["foo"] = want
        self.assertEqual(want, m["foo"])
        self.assertEqual(
            [("bar/baz/qux.py", BIN_HASH_2), ("foo", BIN_HASH_1 + b"a")],
            list(m.items()),
        )
        # Sometimes it even tries a 22-byte fake hash, but we can
        # return 21 and it'll work out
        m["foo"] = want + b"+"
        self.assertEqual(want, m["foo"])
        # make sure the suffix survives a copy
        match = matchmod.match(os.getcwd(), "", ["re:foo"])
        m2 = m.matches(match)
        self.assertEqual(want, m2["foo"])
        self.assertEqual(1, len(m2))
        m2 = m.copy()
        self.assertEqual(want, m2["foo"])
        # suffix with iteration
        self.assertEqual(
            [("bar/baz/qux.py", BIN_HASH_2), ("foo", want)], list(m.items())
        )

        # shows up in diff
        self.assertEqual({"foo": ((want, f), (h, ""))}, m.diff(clean))
        self.assertEqual({"foo": ((h, ""), (want, f))}, clean.diff(m))

    def testMatchException(self):
        m = self.parsemanifest(A_SHORT_MANIFEST)
        match = matchmod.match(os.getcwd(), "", ["re:.*"])

        def filt(path):
            if path == "foo":
                assert False
            return True

        match.matchfn = filt
        with self.assertRaises(AssertionError):
            m.matches(match)

    def testRemoveItem(self):
        m = self.parsemanifest(A_SHORT_MANIFEST)
        del m["foo"]
        with self.assertRaises(KeyError):
            m["foo"]
        self.assertEqual(1, len(m))
        self.assertEqual(1, len(list(m)))
        # now restore and make sure everything works right
        m["foo"] = b"a" * 20
        self.assertEqual(2, len(m))
        self.assertEqual(2, len(list(m)))

    def testManifestDiff(self):
        MISSING = (None, "")
        addl = b"z-only-in-left\0" + HASH_1 + b"\n"
        addr = b"z-only-in-right\0" + HASH_2 + b"x\n"
        left = self.parsemanifest(
            A_SHORT_MANIFEST.replace(HASH_1, HASH_3 + b"x") + addl
        )
        right = self.parsemanifest(A_SHORT_MANIFEST + addr)
        want = {
            "foo": ((BIN_HASH_3, "x"), (BIN_HASH_1, "")),
            "z-only-in-left": ((BIN_HASH_1, ""), MISSING),
            "z-only-in-right": (MISSING, (BIN_HASH_2, "x")),
        }
        self.assertEqual(want, left.diff(right))

        want = {
            "bar/baz/qux.py": (MISSING, (BIN_HASH_2, "l")),
            "foo": (MISSING, (BIN_HASH_3, "x")),
            "z-only-in-left": (MISSING, (BIN_HASH_1, "")),
        }
        self.assertEqual(want, self.parsemanifest(EMPTY_MANIFEST).diff(left))

        want = {
            "bar/baz/qux.py": ((BIN_HASH_2, "l"), MISSING),
            "foo": ((BIN_HASH_3, "x"), MISSING),
            "z-only-in-left": ((BIN_HASH_1, ""), MISSING),
        }
        self.assertEqual(want, left.diff(self.parsemanifest(EMPTY_MANIFEST)))
        copy = right.copy()
        del copy["z-only-in-right"]
        del right["foo"]
        want = {
            "foo": (MISSING, (BIN_HASH_1, "")),
            "z-only-in-right": ((BIN_HASH_2, "x"), MISSING),
        }
        self.assertEqual(want, right.diff(copy))

        short = self.parsemanifest(A_SHORT_MANIFEST)
        pruned = short.copy()
        del pruned["foo"]
        want = {"foo": ((BIN_HASH_1, ""), MISSING)}
        self.assertEqual(want, short.diff(pruned))
        want = {"foo": (MISSING, (BIN_HASH_1, ""))}
        self.assertEqual(want, pruned.diff(short))

    def testReversedLines(self):
        backwards = b"".join(
            l + b"\n" for l in reversed(A_SHORT_MANIFEST.split(b"\n")) if l
        )
        try:
            self.parsemanifest(backwards)
            self.fail("Should have raised ValueError")
        except ValueError as v:
            self.assertIn("Manifest lines not in sorted order.", str(v))

    def testNoTerminalNewline(self):
        try:
            self.parsemanifest(A_SHORT_MANIFEST + b"wat")
            self.fail("Should have raised ValueError")
        except ValueError as v:
            self.assertIn("Manifest did not end in a newline.", str(v))

    def testNoNewLineAtAll(self):
        try:
            self.parsemanifest(b"wat")
            self.fail("Should have raised ValueError")
        except ValueError as v:
            self.assertIn("Manifest did not end in a newline.", str(v))

    def testHugeManifest(self):
        m = self.parsemanifest(A_HUGE_MANIFEST)
        self.assertEqual(HUGE_MANIFEST_ENTRIES, len(m))
        self.assertEqual(len(m), len(list(m)))

    def testMatchesMetadata(self):
        """Tests matches() for a few specific files to make sure that both
        the set of files as well as their flags and nodeids are correct in
        the resulting manifest."""
        m = self.parsemanifest(A_HUGE_MANIFEST)

        match = matchmod.match("/", "", ["file1", "file200", "file300"])
        m2 = m.matches(match)

        w = (b"file1\0%sx\nfile200\0%sl\nfile300\0%s\n") % (
            HASH_2,
            HASH_1,
            HASH_1,
        )
        self.assertEqual(w, m2.text())

    def testMatchesNonexistentFile(self):
        """Tests matches() for a small set of specific files, including one
        nonexistent file to make sure in only matches against existing files.
        """
        m = self.parsemanifest(A_DEEPER_MANIFEST)

        match = matchmod.match(
            "/",
            "",
            ["a/b/c/bar.txt", "a/b/d/qux.py", "readme.txt", "nonexistent"],
        )
        m2 = m.matches(match)

        self.assertEqual(["a/b/c/bar.txt", "a/b/d/qux.py", "readme.txt"], m2.keys())

    def testMatchesNonexistentDirectory(self):
        """Tests matches() for a relpath match on a directory that doesn't
        actually exist."""
        m = self.parsemanifest(A_DEEPER_MANIFEST)

        match = matchmod.match("/", "", ["a/f"], default="relpath")
        m2 = m.matches(match)

        self.assertEqual([], m2.keys())

    def testMatchesExactLarge(self):
        """Tests matches() for files matching a large list of exact files."""
        m = self.parsemanifest(A_HUGE_MANIFEST)

        flist = m.keys()[80:300]
        match = matchmod.match("/", "", flist)
        m2 = m.matches(match)

        self.assertEqual(flist, m2.keys())

    def testMatchesFull(self):
        """Tests matches() for what should be a full match."""
        m = self.parsemanifest(A_DEEPER_MANIFEST)

        match = matchmod.match("/", "", [""], default="path")
        m2 = m.matches(match)

        self.assertEqual(m.keys(), m2.keys())

    def testMatchesDirectory(self):
        """Tests matches() on a relpath match on a directory, which should
        match against all files within said directory."""
        m = self.parsemanifest(A_DEEPER_MANIFEST)

        match = matchmod.match("/", "", ["a/b"], default="relpath")
        m2 = m.matches(match)

        self.assertEqual(
            [
                "a/b/c/bar.py",
                "a/b/c/bar.txt",
                "a/b/c/foo.py",
                "a/b/c/foo.txt",
                "a/b/d/baz.py",
                "a/b/d/qux.py",
                "a/b/d/ten.txt",
                "a/b/dog.py",
                "a/b/fish.py",
            ],
            m2.keys(),
        )

    def testMatchesExactPath(self):
        """Tests matches() on an exact match on a directory, which should
        result in an empty manifest because you can't perform an exact match
        against a directory."""
        m = self.parsemanifest(A_DEEPER_MANIFEST)

        match = matchmod.match("/", "", ["a/b"])
        m2 = m.matches(match)

        self.assertEqual([], m2.keys())

    def testMatchesCwd(self):
        """Tests matches() on a relpath match with the current directory ('.')
        when not in the root directory."""
        m = self.parsemanifest(A_DEEPER_MANIFEST)

        match = matchmod.match("/", "a/b", ["."], default="relpath")
        m2 = m.matches(match)

        self.assertEqual(
            [
                "a/b/c/bar.py",
                "a/b/c/bar.txt",
                "a/b/c/foo.py",
                "a/b/c/foo.txt",
                "a/b/d/baz.py",
                "a/b/d/qux.py",
                "a/b/d/ten.txt",
                "a/b/dog.py",
                "a/b/fish.py",
            ],
            m2.keys(),
        )

    def testMatchesWithPattern(self):
        """Tests matches() for files matching a pattern that reside
        deeper than the specified directory."""
        m = self.parsemanifest(A_DEEPER_MANIFEST)

        match = matchmod.match("/", "", ["a/b/*/*.txt"])
        m2 = m.matches(match)

        self.assertEqual(["a/b/c/bar.txt", "a/b/c/foo.txt", "a/b/d/ten.txt"], m2.keys())


class testmanifestdict(unittest.TestCase, basemanifesttests):
    def parsemanifest(self, text):
        return manifestmod.manifestdict(text)


if __name__ == "__main__":
    silenttestrunner.main(__name__)
