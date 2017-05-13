from __future__ import absolute_import

import glob
import os
import shutil
import tempfile
import unittest

from mercurial import (
    util,
)
atomictempfile = util.atomictempfile

class testatomictempfile(unittest.TestCase):
    def setUp(self):
        self._testdir = tempfile.mkdtemp('atomictempfiletest')
        self._filename = os.path.join(self._testdir, 'testfilename')

    def tearDown(self):
        shutil.rmtree(self._testdir, True)

    def testsimple(self):
        file = atomictempfile(self._filename)
        self.assertFalse(os.path.isfile(self._filename))
        tempfilename = file._tempname
        self.assertTrue(tempfilename in glob.glob(
            os.path.join(self._testdir, '.testfilename-*')))

        file.write(b'argh\n')
        file.close()

        self.assertTrue(os.path.isfile(self._filename))
        self.assertTrue(tempfilename not in glob.glob(
            os.path.join(self._testdir, '.testfilename-*')))

    # discard() removes the temp file without making the write permanent
    def testdiscard(self):
        file = atomictempfile(self._filename)
        (dir, basename) = os.path.split(file._tempname)

        file.write(b'yo\n')
        file.discard()

        self.assertFalse(os.path.isfile(self._filename))
        self.assertTrue(basename not in os.listdir('.'))

    # if a programmer screws up and passes bad args to atomictempfile, they
    # get a plain ordinary TypeError, not infinite recursion
    def testoops(self):
        with self.assertRaises(TypeError):
            atomictempfile()

    # checkambig=True avoids ambiguity of timestamp
    def testcheckambig(self):
        def atomicwrite(checkambig):
            f = atomictempfile(self._filename, checkambig=checkambig)
            f.write('FOO')
            f.close()

        # try some times, because reproduction of ambiguity depends on
        # "filesystem time"
        for i in xrange(5):
            atomicwrite(False)
            oldstat = os.stat(self._filename)
            if oldstat.st_ctime != oldstat.st_mtime:
                # subsequent changing never causes ambiguity
                continue

            repetition = 3

            # repeat atomic write with checkambig=True, to examine
            # whether st_mtime is advanced multiple times as expected
            for j in xrange(repetition):
                atomicwrite(True)
            newstat = os.stat(self._filename)
            if oldstat.st_ctime != newstat.st_ctime:
                # timestamp ambiguity was naturally avoided while repetition
                continue

            # st_mtime should be advanced "repetition" times, because
            # all atomicwrite() occurred at same time (in sec)
            self.assertTrue(newstat.st_mtime ==
                            ((oldstat.st_mtime + repetition) & 0x7fffffff))
            # no more examination is needed, if assumption above is true
            break
        else:
            # This platform seems too slow to examine anti-ambiguity
            # of file timestamp (or test happened to be executed at
            # bad timing). Exit silently in this case, because running
            # on other faster platforms can detect problems
            pass

    def testread(self):
        with open(self._filename, 'wb') as f:
            f.write(b'foobar\n')
        file = atomictempfile(self._filename, mode='rb')
        self.assertTrue(file.read(), b'foobar\n')
        file.discard()

    def testcontextmanagersuccess(self):
        """When the context closes, the file is closed"""
        with atomictempfile('foo') as f:
            self.assertFalse(os.path.isfile('foo'))
            f.write(b'argh\n')
        self.assertTrue(os.path.isfile('foo'))

    def testcontextmanagerfailure(self):
        """On exception, the file is discarded"""
        try:
            with atomictempfile('foo') as f:
                self.assertFalse(os.path.isfile('foo'))
                f.write(b'argh\n')
                raise ValueError
        except ValueError:
            pass
        self.assertFalse(os.path.isfile('foo'))

if __name__ == '__main__':
    import silenttestrunner
    silenttestrunner.main(__name__)
