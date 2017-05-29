from __future__ import absolute_import

import binascii
import itertools
import silenttestrunner
import unittest

from mercurial import (
    manifest as manifestmod,
    match as matchmod,
)

EMTPY_MANIFEST = b''
EMTPY_MANIFEST_V2 = b'\0\n'

HASH_1 = b'1' * 40
BIN_HASH_1 = binascii.unhexlify(HASH_1)
HASH_2 = b'f' * 40
BIN_HASH_2 = binascii.unhexlify(HASH_2)
HASH_3 = b'1234567890abcdef0987654321deadbeef0fcafe'
BIN_HASH_3 = binascii.unhexlify(HASH_3)
A_SHORT_MANIFEST = (
    b'bar/baz/qux.py\0%(hash2)s%(flag2)s\n'
    b'foo\0%(hash1)s%(flag1)s\n'
    ) % {b'hash1': HASH_1,
         b'flag1': b'',
         b'hash2': HASH_2,
         b'flag2': b'l',
         }

# Same data as A_SHORT_MANIFEST
A_SHORT_MANIFEST_V2 = (
    b'\0\n'
    b'\x00bar/baz/qux.py\0%(flag2)s\n%(hash2)s\n'
    b'\x00foo\0%(flag1)s\n%(hash1)s\n'
    ) % {b'hash1': BIN_HASH_1,
         b'flag1': b'',
         b'hash2': BIN_HASH_2,
         b'flag2': b'l',
         }

# Same data as A_SHORT_MANIFEST
A_METADATA_MANIFEST = (
    b'\0foo\0bar\n'
    b'\x00bar/baz/qux.py\0%(flag2)s\0foo\0bar\n%(hash2)s\n' # flag and metadata
    b'\x00foo\0%(flag1)s\0foo\n%(hash1)s\n' # no flag, but metadata
    ) % {b'hash1': BIN_HASH_1,
         b'flag1': b'',
         b'hash2': BIN_HASH_2,
         b'flag2': b'l',
         }

A_STEM_COMPRESSED_MANIFEST = (
    b'\0\n'
    b'\x00bar/baz/qux.py\0%(flag2)s\n%(hash2)s\n'
    b'\x04qux/foo.py\0%(flag1)s\n%(hash1)s\n' # simple case of 4 stem chars
    b'\x0az.py\0%(flag1)s\n%(hash1)s\n' # tricky newline = 10 stem characters
    b'\x00%(verylongdir)sx/x\0\n%(hash1)s\n'
    b'\xffx/y\0\n%(hash2)s\n' # more than 255 stem chars
    ) % {b'hash1': BIN_HASH_1,
         b'flag1': b'',
         b'hash2': BIN_HASH_2,
         b'flag2': b'l',
         b'verylongdir': 255 * b'x',
         }

A_DEEPER_MANIFEST = (
    b'a/b/c/bar.py\0%(hash3)s%(flag1)s\n'
    b'a/b/c/bar.txt\0%(hash1)s%(flag1)s\n'
    b'a/b/c/foo.py\0%(hash3)s%(flag1)s\n'
    b'a/b/c/foo.txt\0%(hash2)s%(flag2)s\n'
    b'a/b/d/baz.py\0%(hash3)s%(flag1)s\n'
    b'a/b/d/qux.py\0%(hash1)s%(flag2)s\n'
    b'a/b/d/ten.txt\0%(hash3)s%(flag2)s\n'
    b'a/b/dog.py\0%(hash3)s%(flag1)s\n'
    b'a/b/fish.py\0%(hash2)s%(flag1)s\n'
    b'a/c/london.py\0%(hash3)s%(flag2)s\n'
    b'a/c/paper.txt\0%(hash2)s%(flag2)s\n'
    b'a/c/paris.py\0%(hash2)s%(flag1)s\n'
    b'a/d/apple.py\0%(hash3)s%(flag1)s\n'
    b'a/d/pizza.py\0%(hash3)s%(flag2)s\n'
    b'a/green.py\0%(hash1)s%(flag2)s\n'
    b'a/purple.py\0%(hash2)s%(flag1)s\n'
    b'app.py\0%(hash3)s%(flag1)s\n'
    b'readme.txt\0%(hash2)s%(flag1)s\n'
    ) % {b'hash1': HASH_1,
         b'flag1': b'',
         b'hash2': HASH_2,
         b'flag2': b'l',
         b'hash3': HASH_3,
         }

HUGE_MANIFEST_ENTRIES = 200001

izip = getattr(itertools, 'izip', zip)
if 'xrange' not in globals():
    xrange = range

A_HUGE_MANIFEST = b''.join(sorted(
    b'file%d\0%s%s\n' % (i, h, f) for i, h, f in
    izip(xrange(200001),
         itertools.cycle((HASH_1, HASH_2)),
         itertools.cycle((b'', b'x', b'l')))))

class basemanifesttests(object):
    def parsemanifest(self, text):
        raise NotImplementedError('parsemanifest not implemented by test case')

    def testEmptyManifest(self):
        m = self.parsemanifest(EMTPY_MANIFEST)
        self.assertEqual(0, len(m))
        self.assertEqual([], list(m))

    def testEmptyManifestv2(self):
        m = self.parsemanifest(EMTPY_MANIFEST_V2)
        self.assertEqual(0, len(m))
        self.assertEqual([], list(m))

    def testManifest(self):
        m = self.parsemanifest(A_SHORT_MANIFEST)
        self.assertEqual([b'bar/baz/qux.py', b'foo'], list(m))
        self.assertEqual(BIN_HASH_2, m[b'bar/baz/qux.py'])
        self.assertEqual(b'l', m.flags(b'bar/baz/qux.py'))
        self.assertEqual(BIN_HASH_1, m[b'foo'])
        self.assertEqual(b'', m.flags(b'foo'))
        with self.assertRaises(KeyError):
            m[b'wat']

    def testParseManifestV2(self):
        m1 = self.parsemanifest(A_SHORT_MANIFEST)
        m2 = self.parsemanifest(A_SHORT_MANIFEST_V2)
        # Should have same content as A_SHORT_MANIFEST
        self.assertEqual(m1.text(), m2.text())

    def testParseManifestMetadata(self):
        # Metadata is for future-proofing and should be accepted but ignored
        m = self.parsemanifest(A_METADATA_MANIFEST)
        self.assertEqual(A_SHORT_MANIFEST, m.text())

    def testParseManifestStemCompression(self):
        m = self.parsemanifest(A_STEM_COMPRESSED_MANIFEST)
        self.assertIn(b'bar/baz/qux.py', m)
        self.assertIn(b'bar/qux/foo.py', m)
        self.assertIn(b'bar/qux/foz.py', m)
        self.assertIn(256 * b'x' + b'/x', m)
        self.assertIn(256 * b'x' + b'/y', m)
        self.assertEqual(A_STEM_COMPRESSED_MANIFEST, m.text(usemanifestv2=True))

    def testTextV2(self):
        m1 = self.parsemanifest(A_SHORT_MANIFEST)
        v2text = m1.text(usemanifestv2=True)
        self.assertEqual(A_SHORT_MANIFEST_V2, v2text)

    def testSetItem(self):
        want = BIN_HASH_1

        m = self.parsemanifest(EMTPY_MANIFEST)
        m[b'a'] = want
        self.assertIn(b'a', m)
        self.assertEqual(want, m[b'a'])
        self.assertEqual(b'a\0' + HASH_1 + b'\n', m.text())

        m = self.parsemanifest(A_SHORT_MANIFEST)
        m[b'a'] = want
        self.assertEqual(want, m[b'a'])
        self.assertEqual(b'a\0' + HASH_1 + b'\n' + A_SHORT_MANIFEST,
                         m.text())

    def testSetFlag(self):
        want = b'x'

        m = self.parsemanifest(EMTPY_MANIFEST)
        # first add a file; a file-less flag makes no sense
        m[b'a'] = BIN_HASH_1
        m.setflag(b'a', want)
        self.assertEqual(want, m.flags(b'a'))
        self.assertEqual(b'a\0' + HASH_1 + want + b'\n', m.text())

        m = self.parsemanifest(A_SHORT_MANIFEST)
        # first add a file; a file-less flag makes no sense
        m[b'a'] = BIN_HASH_1
        m.setflag(b'a', want)
        self.assertEqual(want, m.flags(b'a'))
        self.assertEqual(b'a\0' + HASH_1 + want + b'\n' + A_SHORT_MANIFEST,
                         m.text())

    def testCopy(self):
        m = self.parsemanifest(A_SHORT_MANIFEST)
        m[b'a'] = BIN_HASH_1
        m2 = m.copy()
        del m
        del m2 # make sure we don't double free() anything

    def testCompaction(self):
        unhex = binascii.unhexlify
        h1, h2 = unhex(HASH_1), unhex(HASH_2)
        m = self.parsemanifest(A_SHORT_MANIFEST)
        m[b'alpha'] = h1
        m[b'beta'] = h2
        del m[b'foo']
        want = b'alpha\0%s\nbar/baz/qux.py\0%sl\nbeta\0%s\n' % (
            HASH_1, HASH_2, HASH_2)
        self.assertEqual(want, m.text())
        self.assertEqual(3, len(m))
        self.assertEqual([b'alpha', b'bar/baz/qux.py', b'beta'], list(m))
        self.assertEqual(h1, m[b'alpha'])
        self.assertEqual(h2, m[b'bar/baz/qux.py'])
        self.assertEqual(h2, m[b'beta'])
        self.assertEqual(b'', m.flags(b'alpha'))
        self.assertEqual(b'l', m.flags(b'bar/baz/qux.py'))
        self.assertEqual(b'', m.flags(b'beta'))
        with self.assertRaises(KeyError):
            m[b'foo']

    def testSetGetNodeSuffix(self):
        clean = self.parsemanifest(A_SHORT_MANIFEST)
        m = self.parsemanifest(A_SHORT_MANIFEST)
        h = m[b'foo']
        f = m.flags(b'foo')
        want = h + b'a'
        # Merge code wants to set 21-byte fake hashes at times
        m[b'foo'] = want
        self.assertEqual(want, m[b'foo'])
        self.assertEqual([(b'bar/baz/qux.py', BIN_HASH_2),
                          (b'foo', BIN_HASH_1 + b'a')],
                         list(m.iteritems()))
        # Sometimes it even tries a 22-byte fake hash, but we can
        # return 21 and it'll work out
        m[b'foo'] = want + b'+'
        self.assertEqual(want, m[b'foo'])
        # make sure the suffix survives a copy
        match = matchmod.match(b'', b'', [b're:foo'])
        m2 = m.matches(match)
        self.assertEqual(want, m2[b'foo'])
        self.assertEqual(1, len(m2))
        m2 = m.copy()
        self.assertEqual(want, m2[b'foo'])
        # suffix with iteration
        self.assertEqual([(b'bar/baz/qux.py', BIN_HASH_2),
                          (b'foo', want)],
                         list(m.iteritems()))

        # shows up in diff
        self.assertEqual({b'foo': ((want, f), (h, b''))}, m.diff(clean))
        self.assertEqual({b'foo': ((h, b''), (want, f))}, clean.diff(m))

    def testMatchException(self):
        m = self.parsemanifest(A_SHORT_MANIFEST)
        match = matchmod.match(b'', b'', [b're:.*'])
        def filt(path):
            if path == b'foo':
                assert False
            return True
        match.matchfn = filt
        with self.assertRaises(AssertionError):
            m.matches(match)

    def testRemoveItem(self):
        m = self.parsemanifest(A_SHORT_MANIFEST)
        del m[b'foo']
        with self.assertRaises(KeyError):
            m[b'foo']
        self.assertEqual(1, len(m))
        self.assertEqual(1, len(list(m)))
        # now restore and make sure everything works right
        m[b'foo'] = b'a' * 20
        self.assertEqual(2, len(m))
        self.assertEqual(2, len(list(m)))

    def testManifestDiff(self):
        MISSING = (None, b'')
        addl = b'z-only-in-left\0' + HASH_1 + b'\n'
        addr = b'z-only-in-right\0' + HASH_2 + b'x\n'
        left = self.parsemanifest(
            A_SHORT_MANIFEST.replace(HASH_1, HASH_3 + b'x') + addl)
        right = self.parsemanifest(A_SHORT_MANIFEST + addr)
        want = {
            b'foo': ((BIN_HASH_3, b'x'),
                     (BIN_HASH_1, b'')),
            b'z-only-in-left': ((BIN_HASH_1, b''), MISSING),
            b'z-only-in-right': (MISSING, (BIN_HASH_2, b'x')),
            }
        self.assertEqual(want, left.diff(right))

        want = {
            b'bar/baz/qux.py': (MISSING, (BIN_HASH_2, b'l')),
            b'foo': (MISSING, (BIN_HASH_3, b'x')),
            b'z-only-in-left': (MISSING, (BIN_HASH_1, b'')),
            }
        self.assertEqual(want, self.parsemanifest(EMTPY_MANIFEST).diff(left))

        want = {
            b'bar/baz/qux.py': ((BIN_HASH_2, b'l'), MISSING),
            b'foo': ((BIN_HASH_3, b'x'), MISSING),
            b'z-only-in-left': ((BIN_HASH_1, b''), MISSING),
            }
        self.assertEqual(want, left.diff(self.parsemanifest(EMTPY_MANIFEST)))
        copy = right.copy()
        del copy[b'z-only-in-right']
        del right[b'foo']
        want = {
            b'foo': (MISSING, (BIN_HASH_1, b'')),
            b'z-only-in-right': ((BIN_HASH_2, b'x'), MISSING),
            }
        self.assertEqual(want, right.diff(copy))

        short = self.parsemanifest(A_SHORT_MANIFEST)
        pruned = short.copy()
        del pruned[b'foo']
        want = {
            b'foo': ((BIN_HASH_1, b''), MISSING),
            }
        self.assertEqual(want, short.diff(pruned))
        want = {
            b'foo': (MISSING, (BIN_HASH_1, b'')),
            }
        self.assertEqual(want, pruned.diff(short))
        want = {
            b'bar/baz/qux.py': None,
            b'foo': (MISSING, (BIN_HASH_1, b'')),
            }
        self.assertEqual(want, pruned.diff(short, clean=True))

    def testReversedLines(self):
        backwards = b''.join(
            l + b'\n' for l in reversed(A_SHORT_MANIFEST.split(b'\n')) if l)
        try:
            self.parsemanifest(backwards)
            self.fail('Should have raised ValueError')
        except ValueError as v:
            self.assertIn('Manifest lines not in sorted order.', str(v))

    def testNoTerminalNewline(self):
        try:
            self.parsemanifest(A_SHORT_MANIFEST + b'wat')
            self.fail('Should have raised ValueError')
        except ValueError as v:
            self.assertIn('Manifest did not end in a newline.', str(v))

    def testNoNewLineAtAll(self):
        try:
            self.parsemanifest(b'wat')
            self.fail('Should have raised ValueError')
        except ValueError as v:
            self.assertIn('Manifest did not end in a newline.', str(v))

    def testHugeManifest(self):
        m = self.parsemanifest(A_HUGE_MANIFEST)
        self.assertEqual(HUGE_MANIFEST_ENTRIES, len(m))
        self.assertEqual(len(m), len(list(m)))

    def testMatchesMetadata(self):
        '''Tests matches() for a few specific files to make sure that both
        the set of files as well as their flags and nodeids are correct in
        the resulting manifest.'''
        m = self.parsemanifest(A_HUGE_MANIFEST)

        match = matchmod.match(b'/', b'',
                [b'file1', b'file200', b'file300'], exact=True)
        m2 = m.matches(match)

        w = (b'file1\0%sx\n'
             b'file200\0%sl\n'
             b'file300\0%s\n') % (HASH_2, HASH_1, HASH_1)
        self.assertEqual(w, m2.text())

    def testMatchesNonexistentFile(self):
        '''Tests matches() for a small set of specific files, including one
        nonexistent file to make sure in only matches against existing files.
        '''
        m = self.parsemanifest(A_DEEPER_MANIFEST)

        match = matchmod.match(b'/', b'',
                [b'a/b/c/bar.txt', b'a/b/d/qux.py',
                 b'readme.txt', b'nonexistent'],
                exact=True)
        m2 = m.matches(match)

        self.assertEqual(
                [b'a/b/c/bar.txt', b'a/b/d/qux.py', b'readme.txt'],
                m2.keys())

    def testMatchesNonexistentDirectory(self):
        '''Tests matches() for a relpath match on a directory that doesn't
        actually exist.'''
        m = self.parsemanifest(A_DEEPER_MANIFEST)

        match = matchmod.match(b'/', b'', [b'a/f'], default=b'relpath')
        m2 = m.matches(match)

        self.assertEqual([], m2.keys())

    def testMatchesExactLarge(self):
        '''Tests matches() for files matching a large list of exact files.
        '''
        m = self.parsemanifest(A_HUGE_MANIFEST)

        flist = m.keys()[80:300]
        match = matchmod.match(b'/', b'', flist, exact=True)
        m2 = m.matches(match)

        self.assertEqual(flist, m2.keys())

    def testMatchesFull(self):
        '''Tests matches() for what should be a full match.'''
        m = self.parsemanifest(A_DEEPER_MANIFEST)

        match = matchmod.match(b'/', b'', [b''])
        m2 = m.matches(match)

        self.assertEqual(m.keys(), m2.keys())

    def testMatchesDirectory(self):
        '''Tests matches() on a relpath match on a directory, which should
        match against all files within said directory.'''
        m = self.parsemanifest(A_DEEPER_MANIFEST)

        match = matchmod.match(b'/', b'', [b'a/b'], default=b'relpath')
        m2 = m.matches(match)

        self.assertEqual([
            b'a/b/c/bar.py', b'a/b/c/bar.txt', b'a/b/c/foo.py',
            b'a/b/c/foo.txt',
            b'a/b/d/baz.py', b'a/b/d/qux.py', b'a/b/d/ten.txt', b'a/b/dog.py',
            b'a/b/fish.py'], m2.keys())

    def testMatchesExactPath(self):
        '''Tests matches() on an exact match on a directory, which should
        result in an empty manifest because you can't perform an exact match
        against a directory.'''
        m = self.parsemanifest(A_DEEPER_MANIFEST)

        match = matchmod.match(b'/', b'', [b'a/b'], exact=True)
        m2 = m.matches(match)

        self.assertEqual([], m2.keys())

    def testMatchesCwd(self):
        '''Tests matches() on a relpath match with the current directory ('.')
        when not in the root directory.'''
        m = self.parsemanifest(A_DEEPER_MANIFEST)

        match = matchmod.match(b'/', b'a/b', [b'.'], default=b'relpath')
        m2 = m.matches(match)

        self.assertEqual([
            b'a/b/c/bar.py', b'a/b/c/bar.txt', b'a/b/c/foo.py',
            b'a/b/c/foo.txt', b'a/b/d/baz.py', b'a/b/d/qux.py',
            b'a/b/d/ten.txt', b'a/b/dog.py', b'a/b/fish.py'], m2.keys())

    def testMatchesWithPattern(self):
        '''Tests matches() for files matching a pattern that reside
        deeper than the specified directory.'''
        m = self.parsemanifest(A_DEEPER_MANIFEST)

        match = matchmod.match(b'/', b'', [b'a/b/*/*.txt'])
        m2 = m.matches(match)

        self.assertEqual(
                [b'a/b/c/bar.txt', b'a/b/c/foo.txt', b'a/b/d/ten.txt'],
                m2.keys())

class testmanifestdict(unittest.TestCase, basemanifesttests):
    def parsemanifest(self, text):
        return manifestmod.manifestdict(text)

class testtreemanifest(unittest.TestCase, basemanifesttests):
    def parsemanifest(self, text):
        return manifestmod.treemanifest(b'', text)

    def testWalkSubtrees(self):
        m = self.parsemanifest(A_DEEPER_MANIFEST)

        dirs = [s._dir for s in m.walksubtrees()]
        self.assertEqual(
            sorted([
                b'', b'a/', b'a/c/', b'a/d/', b'a/b/', b'a/b/c/', b'a/b/d/']),
            sorted(dirs)
        )

        match = matchmod.match(b'/', b'', [b'path:a/b/'])
        dirs = [s._dir for s in m.walksubtrees(matcher=match)]
        self.assertEqual(
            sorted([b'a/b/', b'a/b/c/', b'a/b/d/']),
            sorted(dirs)
        )

if __name__ == '__main__':
    silenttestrunner.main(__name__)
