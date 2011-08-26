import os
import glob
from mercurial.util import atomictempfile

# basic usage
def test1_simple():
    if os.path.exists('foo'):
        os.remove('foo')
    file = atomictempfile('foo')
    (dir, basename) = os.path.split(file._tempname)
    assert not os.path.isfile('foo')
    assert basename in glob.glob('.foo-*')

    file.write('argh\n')
    file.close()

    assert os.path.isfile('foo')
    assert basename not in glob.glob('.foo-*')
    print 'OK'

# discard() removes the temp file without making the write permanent
def test2_discard():
    if os.path.exists('foo'):
        os.remove('foo')
    file = atomictempfile('foo')
    (dir, basename) = os.path.split(file._tempname)

    file.write('yo\n')
    file.discard()

    assert not os.path.isfile('foo')
    assert basename not in os.listdir('.')
    print 'OK'

# if a programmer screws up and passes bad args to atomictempfile, they
# get a plain ordinary TypeError, not infinite recursion
def test3_oops():
    try:
        file = atomictempfile()
    except TypeError:
        print "OK"
    else:
        print "expected TypeError"

if __name__ == '__main__':
    test1_simple()
    test2_discard()
    test3_oops()
