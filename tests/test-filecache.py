from __future__ import absolute_import, print_function
import os
import subprocess
import sys

if subprocess.call(['python', '%s/hghave' % os.environ['TESTDIR'],
                    'cacheable']):
    sys.exit(80)

from mercurial import (
    extensions,
    hg,
    localrepo,
    ui as uimod,
    util,
    vfs as vfsmod,
)

class fakerepo(object):
    def __init__(self):
        self._filecache = {}

    class fakevfs(object):

        def join(self, p):
            return p

    vfs = fakevfs()

    def unfiltered(self):
        return self

    def sjoin(self, p):
        return p

    @localrepo.repofilecache('x', 'y')
    def cached(self):
        print('creating')
        return 'string from function'

    def invalidate(self):
        for k in self._filecache:
            try:
                delattr(self, k)
            except AttributeError:
                pass

def basic(repo):
    print("* neither file exists")
    # calls function
    repo.cached

    repo.invalidate()
    print("* neither file still exists")
    # uses cache
    repo.cached

    # create empty file
    f = open('x', 'w')
    f.close()
    repo.invalidate()
    print("* empty file x created")
    # should recreate the object
    repo.cached

    f = open('x', 'w')
    f.write('a')
    f.close()
    repo.invalidate()
    print("* file x changed size")
    # should recreate the object
    repo.cached

    repo.invalidate()
    print("* nothing changed with either file")
    # stats file again, reuses object
    repo.cached

    # atomic replace file, size doesn't change
    # hopefully st_mtime doesn't change as well so this doesn't use the cache
    # because of inode change
    f = vfsmod.vfs('.')('x', 'w', atomictemp=True)
    f.write('b')
    f.close()

    repo.invalidate()
    print("* file x changed inode")
    repo.cached

    # create empty file y
    f = open('y', 'w')
    f.close()
    repo.invalidate()
    print("* empty file y created")
    # should recreate the object
    repo.cached

    f = open('y', 'w')
    f.write('A')
    f.close()
    repo.invalidate()
    print("* file y changed size")
    # should recreate the object
    repo.cached

    f = vfsmod.vfs('.')('y', 'w', atomictemp=True)
    f.write('B')
    f.close()

    repo.invalidate()
    print("* file y changed inode")
    repo.cached

    f = vfsmod.vfs('.')('x', 'w', atomictemp=True)
    f.write('c')
    f.close()
    f = vfsmod.vfs('.')('y', 'w', atomictemp=True)
    f.write('C')
    f.close()

    repo.invalidate()
    print("* both files changed inode")
    repo.cached

def fakeuncacheable():
    def wrapcacheable(orig, *args, **kwargs):
        return False

    def wrapinit(orig, *args, **kwargs):
        pass

    originit = extensions.wrapfunction(util.cachestat, '__init__', wrapinit)
    origcacheable = extensions.wrapfunction(util.cachestat, 'cacheable',
                                            wrapcacheable)

    for fn in ['x', 'y']:
        try:
            os.remove(fn)
        except OSError:
            pass

    basic(fakerepo())

    util.cachestat.cacheable = origcacheable
    util.cachestat.__init__ = originit

def test_filecache_synced():
    # test old behavior that caused filecached properties to go out of sync
    os.system('hg init && echo a >> a && hg ci -qAm.')
    repo = hg.repository(uimod.ui.load())
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
    os.remove('y')
    repo.cached = 'string set externally'
    repo.invalidate()
    print("* neither file exists")
    print(repo.cached)
    repo.invalidate()
    f = open('x', 'w')
    f.write('a')
    f.close()
    print("* file x created")
    print(repo.cached)

    repo.cached = 'string 2 set externally'
    repo.invalidate()
    print("* string set externally again")
    print(repo.cached)

    repo.invalidate()
    f = open('y', 'w')
    f.write('b')
    f.close()
    print("* file y created")
    print(repo.cached)

def antiambiguity():
    filename = 'ambigcheck'

    # try some times, because reproduction of ambiguity depends on
    # "filesystem time"
    for i in xrange(5):
        fp = open(filename, 'w')
        fp.write('FOO')
        fp.close()

        oldstat = os.stat(filename)
        if oldstat.st_ctime != oldstat.st_mtime:
            # subsequent changing never causes ambiguity
            continue

        repetition = 3

        # repeat changing via checkambigatclosing, to examine whether
        # st_mtime is advanced multiple times as expected
        for i in xrange(repetition):
            # explicit closing
            fp = vfsmod.checkambigatclosing(open(filename, 'a'))
            fp.write('FOO')
            fp.close()

            # implicit closing by "with" statement
            with vfsmod.checkambigatclosing(open(filename, 'a')) as fp:
                fp.write('BAR')

        newstat = os.stat(filename)
        if oldstat.st_ctime != newstat.st_ctime:
            # timestamp ambiguity was naturally avoided while repetition
            continue

        # st_mtime should be advanced "repetition * 2" times, because
        # all changes occurred at same time (in sec)
        expected = (oldstat.st_mtime + repetition * 2) & 0x7fffffff
        if newstat.st_mtime != expected:
            print("'newstat.st_mtime %s is not %s (as %s + %s * 2)" %
                  (newstat.st_mtime, expected, oldstat.st_mtime, repetition))

        # no more examination is needed regardless of result
        break
    else:
        # This platform seems too slow to examine anti-ambiguity
        # of file timestamp (or test happened to be executed at
        # bad timing). Exit silently in this case, because running
        # on other faster platforms can detect problems
        pass

print('basic:')
print()
basic(fakerepo())
print()
print('fakeuncacheable:')
print()
fakeuncacheable()
test_filecache_synced()
print()
print('setbeforeget:')
print()
setbeforeget(fakerepo())
print()
print('antiambiguity:')
print()
antiambiguity()
