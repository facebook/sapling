import sys, os, subprocess

if subprocess.call(['%s/hghave' % os.environ['TESTDIR'], 'cacheable']):
    sys.exit(80)

from mercurial import util, scmutil, extensions

filecache = scmutil.filecache

class fakerepo(object):
    def __init__(self):
        self._filecache = {}

    def join(self, p):
        return p

    def sjoin(self, p):
        return p

    @filecache('x')
    def cached(self):
        print 'creating'

    def invalidate(self):
        for k in self._filecache:
            try:
                delattr(self, k)
            except AttributeError:
                pass

def basic(repo):
    # file doesn't exist, calls function
    repo.cached

    repo.invalidate()
    # file still doesn't exist, uses cache
    repo.cached

    # create empty file
    f = open('x', 'w')
    f.close()
    repo.invalidate()
    # should recreate the object
    repo.cached

    f = open('x', 'w')
    f.write('a')
    f.close()
    repo.invalidate()
    # should recreate the object
    repo.cached

    repo.invalidate()
    # stats file again, nothing changed, reuses object
    repo.cached

    # atomic replace file, size doesn't change
    # hopefully st_mtime doesn't change as well so this doesn't use the cache
    # because of inode change
    f = scmutil.opener('.')('x', 'w', atomictemp=True)
    f.write('b')
    f.close()

    repo.invalidate()
    repo.cached

def fakeuncacheable():
    def wrapcacheable(orig, *args, **kwargs):
        return False

    def wrapinit(orig, *args, **kwargs):
        pass

    originit = extensions.wrapfunction(util.cachestat, '__init__', wrapinit)
    origcacheable = extensions.wrapfunction(util.cachestat, 'cacheable',
                                            wrapcacheable)

    try:
        os.remove('x')
    except:
        pass

    basic(fakerepo())

    util.cachestat.cacheable = origcacheable
    util.cachestat.__init__ = originit

print 'basic:'
print
basic(fakerepo())
print
print 'fakeuncacheable:'
print
fakeuncacheable()
