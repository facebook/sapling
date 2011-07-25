import os
from mercurial import hg, ui
from mercurial.scmutil import walkrepos
from os import mkdir, chdir
from os.path import join as pjoin

u = ui.ui()
sym = getattr(os, 'symlink', False) and getattr(os.path, 'samestat', False)

hg.repository(u, 'top1', create=1)
mkdir('subdir')
chdir('subdir')
hg.repository(u, 'sub1', create=1)
mkdir('subsubdir')
chdir('subsubdir')
hg.repository(u, 'subsub1', create=1)
chdir(os.path.pardir)
if sym:
    os.symlink(os.path.pardir, 'circle')
    os.symlink(pjoin('subsubdir', 'subsub1'), 'subsub1')

def runtest():
    reposet = frozenset(walkrepos('.', followsym=True))
    if sym and (len(reposet) != 3):
        print "reposet = %r" % (reposet,)
        print "Found %d repositories when I should have found 3" % (len(reposet),)
    if (not sym) and (len(reposet) != 2):
        print "reposet = %r" % (reposet,)
        print "Found %d repositories when I should have found 2" % (len(reposet),)
    sub1set = frozenset((pjoin('.', 'sub1'),
                         pjoin('.', 'circle', 'subdir', 'sub1')))
    if len(sub1set & reposet) != 1:
        print "sub1set = %r" % (sub1set,)
        print "reposet = %r" % (reposet,)
        print "sub1set and reposet should have exactly one path in common."
    sub2set = frozenset((pjoin('.', 'subsub1'),
                         pjoin('.', 'subsubdir', 'subsub1')))
    if len(sub2set & reposet) != 1:
        print "sub2set = %r" % (sub2set,)
        print "reposet = %r" % (reposet,)
        print "sub1set and reposet should have exactly one path in common."
    sub3 = pjoin('.', 'circle', 'top1')
    if sym and not (sub3 in reposet):
        print "reposet = %r" % (reposet,)
        print "Symbolic links are supported and %s is not in reposet" % (sub3,)

runtest()
if sym:
    # Simulate not having symlinks.
    del os.path.samestat
    sym = False
    runtest()
