from __future__ import absolute_import

import glob
import os
import unittest

from mercurial import (
    util,
)
atomictempfile = util.atomictempfile

class testatomictempfile(unittest.TestCase):
    def test1_simple(self):
        if os.path.exists('foo'):
            os.remove('foo')
        file = atomictempfile('foo')
        (dir, basename) = os.path.split(file._tempname)
        self.assertFalse(os.path.isfile('foo'))
        self.assertTrue(basename in glob.glob('.foo-*'))

        file.write(b'argh\n')
        file.close()

        self.assertTrue(os.path.isfile('foo'))
        self.assertTrue(basename not in glob.glob('.foo-*'))

    # discard() removes the temp file without making the write permanent
    def test2_discard(self):
        if os.path.exists('foo'):
            os.remove('foo')
        file = atomictempfile('foo')
        (dir, basename) = os.path.split(file._tempname)

        file.write(b'yo\n')
        file.discard()

        self.assertFalse(os.path.isfile('foo'))
        self.assertTrue(basename not in os.listdir('.'))

    # if a programmer screws up and passes bad args to atomictempfile, they
    # get a plain ordinary TypeError, not infinite recursion
    def test3_oops(self):
        self.assertRaises(TypeError, atomictempfile)

    # checkambig=True avoids ambiguity of timestamp
    def test4_checkambig(self):
        def atomicwrite(checkambig):
            f = atomictempfile('foo', checkambig=checkambig)
            f.write('FOO')
            f.close()

        # try some times, because reproduction of ambiguity depends on
        # "filesystem time"
        for i in xrange(5):
            atomicwrite(False)
            oldstat = os.stat('foo')
            if oldstat.st_ctime != oldstat.st_mtime:
                # subsequent changing never causes ambiguity
                continue

            repetition = 3

            # repeat atomic write with checkambig=True, to examine
            # whether st_mtime is advanced multiple times as expecetd
            for j in xrange(repetition):
                atomicwrite(True)
            newstat = os.stat('foo')
            if oldstat.st_ctime != newstat.st_ctime:
                # timestamp ambiguity was naturally avoided while repetition
                continue

            # st_mtime should be advanced "repetition" times, because
            # all atomicwrite() occured at same time (in sec)
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

if __name__ == '__main__':
    import silenttestrunner
    silenttestrunner.main(__name__)
