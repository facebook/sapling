# Copyright 2004-present Facebook. All Rights Reserved.

import errno
import os
import sys
import time
import unittest

import silenttestrunner

if __name__ == '__main__':
    sys.path.insert(0, os.path.join(os.environ["TESTDIR"], "..", "hgext3rd"))

import extutil

class ExtutilTests(unittest.TestCase):
    def testbgcommandnoblock(self):
        '''runbgcommand() should return without waiting for the process to
        finish.'''
        env = os.environ.copy()
        start = time.time()
        extutil.runbgcommand(['sleep', '5'], env)
        end = time.time()
        if end - start >= 1.0:
            self.fail('runbgcommand() took took %s seconds, should have '
                      'returned immediately' % (end - start))

    def testbgcommandfailure(self):
        '''runbgcommand() should throw if executing the process fails.'''
        env = os.environ.copy()
        try:
            extutil.runbgcommand(['no_such_program', 'arg1', 'arg2'], env)
            self.fail('expected runbgcommand to fail with ENOENT')
        except OSError as ex:
            self.assertEqual(ex.errno, errno.ENOENT)

    def testbgcommandfailure(self):
        '''runbgcommand() should throw if executing the process fails.'''
        env = os.environ.copy()
        try:
            extutil.runbgcommand([os.devnull, 'arg1', 'arg2'], env)
            self.fail('expected runbgcommand to fail with EACCES')
        except OSError as ex:
            self.assertEqual(ex.errno, errno.EACCES)

if __name__ == '__main__':
    silenttestrunner.main(__name__)
