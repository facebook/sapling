from __future__ import absolute_import

import unittest

import silenttestrunner

from mercurial import (
    match as matchmod,
)

class NeverMatcherTests(unittest.TestCase):

    def testVisitdir(self):
        m = matchmod.nevermatcher('', '')
        self.assertFalse(m.visitdir('.'))
        self.assertFalse(m.visitdir('dir'))

if __name__ == '__main__':
    silenttestrunner.main(__name__)
