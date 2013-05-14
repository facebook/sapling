import os
import glob
import unittest
import silenttestrunner

from mercurial.util import atomictempfile

class testatomictempfile(unittest.TestCase):
    def test1_simple(self):
        if os.path.exists('foo'):
            os.remove('foo')
        file = atomictempfile('foo')
        (dir, basename) = os.path.split(file._tempname)
        self.assertFalse(os.path.isfile('foo'))
        self.assertTrue(basename in glob.glob('.foo-*'))

        file.write('argh\n')
        file.close()

        self.assertTrue(os.path.isfile('foo'))
        self.assertTrue(basename not in glob.glob('.foo-*'))

    # discard() removes the temp file without making the write permanent
    def test2_discard(self):
        if os.path.exists('foo'):
            os.remove('foo')
        file = atomictempfile('foo')
        (dir, basename) = os.path.split(file._tempname)

        file.write('yo\n')
        file.discard()

        self.assertFalse(os.path.isfile('foo'))
        self.assertTrue(basename not in os.listdir('.'))

    # if a programmer screws up and passes bad args to atomictempfile, they
    # get a plain ordinary TypeError, not infinite recursion
    def test3_oops(self):
        self.assertRaises(TypeError, atomictempfile)

if __name__ == '__main__':
    silenttestrunner.main(__name__)
