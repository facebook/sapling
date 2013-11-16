import sys, os, subprocess

if subprocess.call(['python', '%s/hghave' % os.environ['TESTDIR'],
                    'cacheable']):
    sys.exit(80)

from mercurial import util, scmutil, extensions, hg, ui

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
        return 'string from function'

    def invalidate(self):
        for k in self._filecache:
            try:
                delattr(self, k)
            except AttributeError:
                pass

def basic(repo):
    print "* file doesn't exist"
    # calls function
    repo.cached

    repo.invalidate()
    print "* file still doesn't exist"
    # uses cache
    repo.cached

    # create empty file
    f = open('x', 'w')
    f.close()
    repo.invalidate()
    print "* empty file x created"
    # should recreate the object
    repo.cached

    f = open('x', 'w')
    f.write('a')
    f.close()
    repo.invalidate()
    print "* file x changed size"
    # should recreate the object
    repo.cached

    repo.invalidate()
    print "* nothing changed with file x"
    # stats file again, reuses object
    repo.cached

    # atomic replace file, size doesn't change
    # hopefully st_mtime doesn't change as well so this doesn't use the cache
    # because of inode change
    f = scmutil.opener('.')('x', 'w', atomictemp=True)
    f.write('b')
    f.close()

    repo.invalidate()
    print "* file x changed inode"
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
    except OSError:
        pass

    basic(fakerepo())

    util.cachestat.cacheable = origcacheable
    util.cachestat.__init__ = originit

def test_filecache_synced():
    # test old behaviour that caused filecached properties to go out of sync
    os.system('hg init && echo a >> a && hg ci -qAm.')
    repo = hg.repository(ui.ui())
    # first rollback clears the filecache, but changelog to stays in __dict__
    repo.rollback()
    repo.commit('.')
    # second rollback comes along and touches the changelog externally
    # (file is moved)
    repo.rollback()
    # but since changelog isn't under the filecache control anymore, we don't
    # see that it changed, and return the old changelog without reconstructing
    # it
    repo.commit('.')

def setbeforeget(repo):
    os.remove('x')
    repo.cached = 'string set externally'
    repo.invalidate()
    print "* file x doesn't exist"
    print repo.cached
    repo.invalidate()
    f = open('x', 'w')
    f.write('a')
    f.close()
    print "* file x created"
    print repo.cached

print 'basic:'
print
basic(fakerepo())
print
print 'fakeuncacheable:'
print
fakeuncacheable()
test_filecache_synced()
print
print 'setbeforeget:'
print
setbeforeget(fakerepo())
