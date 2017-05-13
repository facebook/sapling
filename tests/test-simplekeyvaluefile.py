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
        # lines need to contain the trailing '\n' to mock the real readlines
        return [l for l in mockfile(path, self).read().splitlines(True)]

    def __call__(self, path, mode, atomictemp):
        return mockfile(path, self)

class testsimplekeyvaluefile(unittest.TestCase):
    def setUp(self):
        self.vfs = mockvfs()

    def testbasicwritingiandreading(self):
        dw = {'key1': 'value1', 'Key2': 'value2'}
        scmutil.simplekeyvaluefile(self.vfs, 'kvfile').write(dw)
        self.assertEqual(sorted(self.vfs.read('kvfile').split('\n')),
                         ['', 'Key2=value2', 'key1=value1'])
        dr = scmutil.simplekeyvaluefile(self.vfs, 'kvfile').read()
        self.assertEqual(dr, dw)

    def testinvalidkeys(self):
        d = {'0key1': 'value1', 'Key2': 'value2'}
        with self.assertRaisesRegexp(error.ProgrammingError,
                                     'keys must start with a letter.*'):
            scmutil.simplekeyvaluefile(self.vfs, 'kvfile').write(d)

        d = {'key1@': 'value1', 'Key2': 'value2'}
        with self.assertRaisesRegexp(error.ProgrammingError, 'invalid key.*'):
            scmutil.simplekeyvaluefile(self.vfs, 'kvfile').write(d)

    def testinvalidvalues(self):
        d = {'key1': 'value1', 'Key2': 'value2\n'}
        with self.assertRaisesRegexp(error.ProgrammingError,  'invalid val.*'):
            scmutil.simplekeyvaluefile(self.vfs, 'kvfile').write(d)

    def testcorruptedfile(self):
        self.vfs.contents['badfile'] = 'ababagalamaga\n'
        with self.assertRaisesRegexp(error.CorruptedState,
                                     'dictionary.*element.*'):
            scmutil.simplekeyvaluefile(self.vfs, 'badfile').read()

    def testfirstline(self):
        dw = {'key1': 'value1'}
        scmutil.simplekeyvaluefile(self.vfs, 'fl').write(dw, firstline='1.0')
        self.assertEqual(self.vfs.read('fl'), '1.0\nkey1=value1\n')
        dr = scmutil.simplekeyvaluefile(self.vfs, 'fl')\
                    .read(firstlinenonkeyval=True)
        self.assertEqual(dr, {'__firstline': '1.0', 'key1': 'value1'})

if __name__ == "__main__":
    silenttestrunner.main(__name__)
