import binascii
import unittest
import itertools

import silenttestrunner

from mercurial import parsers

HASH_1 = '1' * 40
HASH_2 = 'f' * 40
HASH_3 = '1234567890abcdef0987654321deadbeef0fcafe'
A_SHORT_MANIFEST = (
    'bar/baz/qux.py\0%(hash2)s%(flag2)s\n'
    'foo\0%(hash1)s%(flag1)s\n'
    ) % {'hash1': HASH_1,
         'flag1': '',
         'hash2': HASH_2,
         'flag2': 'l',
         }

HUGE_MANIFEST_ENTRIES = 200001

A_HUGE_MANIFEST = ''.join(sorted(
    'file%d\0%s%s\n' % (i, h, f) for i, h, f in
    itertools.izip(xrange(200001),
                   itertools.cycle((HASH_1, HASH_2)),
                   itertools.cycle(('', 'x', 'l')))))

class testmanifest(unittest.TestCase):

    def assertIn(self, thing, container, msg=None):
        # assertIn new in 2.7, use it if available, otherwise polyfill
        sup = getattr(unittest.TestCase, 'assertIn', False)
        if sup:
            return sup(self, thing, container, msg=msg)
        if not msg:
            msg = 'Expected %r in %r' % (thing, container)
        self.assert_(thing in container, msg)

    def testEmptyManifest(self):
        m = parsers.lazymanifest('')
        self.assertEqual(0, len(m))
        self.assertEqual([], list(m))

    def testManifest(self):
        m = parsers.lazymanifest(A_SHORT_MANIFEST)
        want = [
            ('bar/baz/qux.py', binascii.unhexlify(HASH_2), 'l'),
            ('foo', binascii.unhexlify(HASH_1), ''),
            ]
        self.assertEqual(len(want), len(m))
        self.assertEqual(want, list(m))
        self.assertEqual((binascii.unhexlify(HASH_1), ''), m['foo'])
        self.assertRaises(KeyError, lambda : m['wat'])
        self.assertEqual((binascii.unhexlify(HASH_2), 'l'),
                         m['bar/baz/qux.py'])

    def testSetItem(self):
        want = binascii.unhexlify(HASH_1), ''

        m = parsers.lazymanifest('')
        m['a'] = want
        self.assertIn('a', m)
        self.assertEqual(want, m['a'])
        self.assertEqual('a\0' + HASH_1 + '\n', m.text())

        m = parsers.lazymanifest(A_SHORT_MANIFEST)
        m['a'] = want
        self.assertEqual(want, m['a'])
        self.assertEqual('a\0' + HASH_1 + '\n' + A_SHORT_MANIFEST,
                         m.text())
        m2 = m.copy()
        del m
        del m2 # make sure we don't double free() anything

    def testCompaction(self):
        unhex = binascii.unhexlify
        h1, h2 = unhex(HASH_1), unhex(HASH_2)
        m = parsers.lazymanifest(A_SHORT_MANIFEST)
        m['alpha'] = h1, ''
        m['beta'] = h2, ''
        del m['foo']
        want = 'alpha\0%s\nbar/baz/qux.py\0%sl\nbeta\0%s\n' % (
            HASH_1, HASH_2, HASH_2)
        self.assertEqual(want, m.text())
        self.assertEqual(3, len(m))
        self.assertEqual((h1, ''), m['alpha'])
        self.assertEqual((h2, ''), m['beta'])
        self.assertRaises(KeyError, lambda : m['foo'])
        w = [('alpha', h1, ''), ('bar/baz/qux.py', h2, 'l'), ('beta', h2, '')]
        self.assertEqual(w, list(m))

    def testSetGetNodeSuffix(self):
        clean = parsers.lazymanifest(A_SHORT_MANIFEST)
        m = parsers.lazymanifest(A_SHORT_MANIFEST)
        h, f = m['foo']
        want = h + 'a', f
        # Merge code wants to set 21-byte fake hashes at times
        m['foo'] = want
        self.assertEqual(want, m['foo'])
        self.assertEqual([('bar/baz/qux.py', binascii.unhexlify(HASH_2), 'l'),
                          ('foo', binascii.unhexlify(HASH_1) + 'a', '')],
                         list(m))
        # Sometimes it even tries a 22-byte fake hash, but we can
        # return 21 and it'll work out
        m['foo'] = want[0] + '+', f
        self.assertEqual(want, m['foo'])
        # make sure the suffix survives a copy
        m2 = m.filtercopy(lambda x: x == 'foo')
        self.assertEqual(want, m2['foo'])
        self.assertEqual(1, len(m2))
        self.assertEqual(('foo\0%s\n' % HASH_1), m2.text())
        m2 = m.copy()
        self.assertEqual(want, m2['foo'])
        # suffix with iteration
        self.assertEqual([('bar/baz/qux.py', binascii.unhexlify(HASH_2), 'l'),
                          ('foo', want[0], '')], list(m))
        # shows up in diff
        self.assertEqual({'foo': (want, (h, ''))}, m.diff(clean))
        self.assertEqual({'foo': ((h, ''), want)}, clean.diff(m))

    def testFilterCopyException(self):
        m = parsers.lazymanifest(A_SHORT_MANIFEST)
        def filt(path):
            if path == 'foo':
                assert False
            return True
        self.assertRaises(AssertionError, m.filtercopy, filt)

    def testRemoveItem(self):
        m = parsers.lazymanifest(A_SHORT_MANIFEST)
        del m['foo']
        self.assertRaises(KeyError, lambda : m['foo'])
        self.assertEqual(1, len(m))
        self.assertEqual(1, len(list(m)))

    def testManifestDiff(self):
        MISSING = (None, '')
        addl = 'z-only-in-left\0' + HASH_1 + '\n'
        addr = 'z-only-in-right\0' + HASH_2 + 'x\n'
        left = parsers.lazymanifest(
            A_SHORT_MANIFEST.replace(HASH_1, HASH_3 + 'x') + addl)
        right = parsers.lazymanifest(A_SHORT_MANIFEST + addr)
        want = {
            'foo': ((binascii.unhexlify(HASH_3), 'x'),
                    (binascii.unhexlify(HASH_1), '')),
            'z-only-in-left': ((binascii.unhexlify(HASH_1), ''), MISSING),
            'z-only-in-right': (MISSING, (binascii.unhexlify(HASH_2), 'x')),
            }
        self.assertEqual(want, left.diff(right))

        want = {
            'bar/baz/qux.py': (MISSING, (binascii.unhexlify(HASH_2), 'l')),
            'foo': (MISSING, (binascii.unhexlify(HASH_3), 'x')),
            'z-only-in-left': (MISSING, (binascii.unhexlify(HASH_1), '')),
            }
        self.assertEqual(want, parsers.lazymanifest('').diff(left))

        want = {
            'bar/baz/qux.py': ((binascii.unhexlify(HASH_2), 'l'), MISSING),
            'foo': ((binascii.unhexlify(HASH_3), 'x'), MISSING),
            'z-only-in-left': ((binascii.unhexlify(HASH_1), ''), MISSING),
            }
        self.assertEqual(want, left.diff(parsers.lazymanifest('')))
        copy = right.copy()
        del copy['z-only-in-right']
        del right['foo']
        want = {
            'foo': (MISSING, (binascii.unhexlify(HASH_1), '')),
            'z-only-in-right': ((binascii.unhexlify(HASH_2), 'x'), MISSING),
            }
        self.assertEqual(want, right.diff(copy))

        short = parsers.lazymanifest(A_SHORT_MANIFEST)
        pruned = short.copy()
        del pruned['foo']
        want = {
            'foo': ((binascii.unhexlify(HASH_1), ''), MISSING),
            }
        self.assertEqual(want, short.diff(pruned))
        want = {
            'foo': (MISSING, (binascii.unhexlify(HASH_1), '')),
            }
        self.assertEqual(want, pruned.diff(short))
        want = {
            'bar/baz/qux.py': None,
            'foo': (MISSING, (binascii.unhexlify(HASH_1), '')),
            }
        self.assertEqual(want, pruned.diff(short, True))

    def testReversedLines(self):
        backwards = ''.join(
            l + '\n' for l in reversed(A_SHORT_MANIFEST.split('\n')) if l)
        try:
            parsers.lazymanifest(backwards)
            self.fail('Should have raised ValueError')
        except ValueError, v:
            self.assertIn('Manifest lines not in sorted order.', str(v))

    def testNoTerminalNewline(self):
        try:
            parsers.lazymanifest(A_SHORT_MANIFEST + 'wat')
            self.fail('Should have raised ValueError')
        except ValueError, v:
            self.assertIn('Manifest did not end in a newline.', str(v))

    def testNoNewLineAtAll(self):
        try:
            parsers.lazymanifest('wat')
            self.fail('Should have raised ValueError')
        except ValueError, v:
            self.assertIn('Manifest did not end in a newline.', str(v))

    def testHugeManifest(self):
        m = parsers.lazymanifest(A_HUGE_MANIFEST)
        self.assertEqual(HUGE_MANIFEST_ENTRIES, len(m))
        self.assertEqual(len(m), len(list(m)))


if __name__ == '__main__':
    silenttestrunner.main(__name__)
