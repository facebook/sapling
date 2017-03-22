from __future__ import absolute_import

import unittest
import silenttestrunner

from mercurial import (
    error,
    scmutil,
)

class mockfile(object):
    def __init__(self, name, fs):
        self.name = name
        self.fs = fs

    def __enter__(self):
        return self

    def __exit__(self, *args, **kwargs):
        pass

    def write(self, text):
        self.fs.contents[self.name] = text

    def read(self):
        return self.fs.contents[self.name]

class mockvfs(object):
    def __init__(self):
        self.contents = {}

    def read(self, path):
        return mockfile(path, self).read()

    def readlines(self, path):
        return mockfile(path, self).read().split('\n')

    def __call__(self, path, mode, atomictemp):
        return mockfile(path, self)

class testsimplekeyvaluefile(unittest.TestCase):
    def setUp(self):
        self.vfs = mockvfs()

    def testbasicwriting(self):
        d = {'key1': 'value1', 'Key2': 'value2'}
        scmutil.simplekeyvaluefile(self.vfs, 'kvfile').write(d)
        self.assertEqual(sorted(self.vfs.read('kvfile').split('\n')),
                         ['', 'Key2=value2', 'key1=value1'])

    def testinvalidkeys(self):
        d = {'0key1': 'value1', 'Key2': 'value2'}
        self.assertRaises(error.ProgrammingError,
                          scmutil.simplekeyvaluefile(self.vfs, 'kvfile').write,
                          d)
        d = {'key1@': 'value1', 'Key2': 'value2'}
        self.assertRaises(error.ProgrammingError,
                          scmutil.simplekeyvaluefile(self.vfs, 'kvfile').write,
                          d)

    def testinvalidvalues(self):
        d = {'key1': 'value1', 'Key2': 'value2\n'}
        self.assertRaises(error.ProgrammingError,
                          scmutil.simplekeyvaluefile(self.vfs, 'kvfile').write,
                          d)

    def testcorruptedfile(self):
        self.vfs.contents['badfile'] = 'ababagalamaga\n'
        self.assertRaises(error.CorruptedState,
                          scmutil.simplekeyvaluefile(self.vfs, 'badfile').read)

if __name__ == "__main__":
    silenttestrunner.main(__name__)
