from __future__ import absolute_import

import unittest

from mercurial import (
    encoding,
)

class IsasciistrTest(unittest.TestCase):
    asciistrs = [
        b'a',
        b'ab',
        b'abc',
        b'abcd',
        b'abcde',
        b'abcdefghi',
        b'abcd\0fghi',
    ]

    def testascii(self):
        for s in self.asciistrs:
            self.assertTrue(encoding.isasciistr(s))

    def testnonasciichar(self):
        for s in self.asciistrs:
            for i in range(len(s)):
                t = bytearray(s)
                t[i] |= 0x80
                self.assertFalse(encoding.isasciistr(bytes(t)))

if __name__ == '__main__':
    import silenttestrunner
    silenttestrunner.main(__name__)
