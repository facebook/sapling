"""test behavior of propertycache and unfiltered propertycache

The repoview overlay is quite complexe. We test the behavior of
property cache of both localrepo and repoview to prevent
regression."""

import os, subprocess
import mercurial.localrepo
import mercurial.repoview
import mercurial.util
import mercurial.hg
import mercurial.ui as uimod


# create some special property cache that trace they call

calllog = []
@mercurial.util.propertycache
def testcachedfoobar(repo):
    name = repo.filtername
    if name is None:
        name = ''
    val = len(name)
    calllog.append(val)
    return val

#plug them on repo
mercurial.localrepo.localrepository.testcachedfoobar = testcachedfoobar


# create an empty repo. and instanciate it. It is important to run
# those test on the real object to detect regression.
repopath = os.path.join(os.environ['TESTTMP'], 'repo')
subprocess.check_call(['hg', 'init', repopath])
ui = uimod.ui()
repo = mercurial.hg.repository(ui, path=repopath).unfiltered()


print ''
print '=== property cache ==='
print ''
print 'calllog:', calllog
print 'cached value (unfiltered):',
print vars(repo).get('testcachedfoobar', 'NOCACHE')

print ''
print '= first access on unfiltered, should do a call'
print 'access:', repo.testcachedfoobar
print 'calllog:', calllog
print 'cached value (unfiltered):',
print vars(repo).get('testcachedfoobar', 'NOCACHE')

print ''
print '= second access on unfiltered, should not do call'
print 'access', repo.testcachedfoobar
print 'calllog:', calllog
print 'cached value (unfiltered):',
print vars(repo).get('testcachedfoobar', 'NOCACHE')

print ''
print '= first access on "visible" view, should do a call'
visibleview = repo.filtered('visible')
print 'cached value ("visible" view):',
print vars(visibleview).get('testcachedfoobar', 'NOCACHE')
print 'access:', visibleview.testcachedfoobar
print 'calllog:', calllog
print 'cached value (unfiltered):',
print vars(repo).get('testcachedfoobar', 'NOCACHE')
print 'cached value ("visible" view):',
print vars(visibleview).get('testcachedfoobar', 'NOCACHE')

print ''
print '= second access on "visible view", should not do call'
print 'access:', visibleview.testcachedfoobar
print 'calllog:', calllog
print 'cached value (unfiltered):',
print vars(repo).get('testcachedfoobar', 'NOCACHE')
print 'cached value ("visible" view):',
print vars(visibleview).get('testcachedfoobar', 'NOCACHE')

print ''
print '= no effect on other view'
immutableview = repo.filtered('immutable')
print 'cached value ("immutable" view):',
print vars(immutableview).get('testcachedfoobar', 'NOCACHE')
print 'access:', immutableview.testcachedfoobar
print 'calllog:', calllog
print 'cached value (unfiltered):',
print vars(repo).get('testcachedfoobar', 'NOCACHE')
print 'cached value ("visible" view):',
print vars(visibleview).get('testcachedfoobar', 'NOCACHE')
print 'cached value ("immutable" view):',
print vars(immutableview).get('testcachedfoobar', 'NOCACHE')

