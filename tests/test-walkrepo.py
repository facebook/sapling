import os
import os.path
from mercurial import hg, ui
from mercurial.util import walkrepos, set, frozenset
from os import mkdir, chdir
from os.path import join as pjoin

u = ui.ui()
sym = hasattr(os, 'symlink') and hasattr(os.path, 'samestat')

hg.repository(u, 'top1', create=1)
mkdir('subdir')
chdir('subdir')
hg.repository(u, 'sub1', create=1)
chdir('sub1')
hg.repository(u, 'inside_sub1', create=1)
chdir('.hg')
hg.repository(u, 'patches', create=1)
chdir(os.path.pardir)
chdir(os.path.pardir)
mkdir('subsubdir')
chdir('subsubdir')
hg.repository(u, 'subsub1', create=1)
chdir(os.path.pardir)
if sym:
    os.symlink(os.path.pardir, 'circle')
    os.symlink(pjoin('subsubdir', 'subsub1'), 'subsub1')

def runtest():
    reposet = frozenset(walkrepos('.', followsym=True))
    if sym and (len(reposet) != 5):
        print "reposet = %r" % (reposet,)
        raise SystemExit(1, "Found %d repositories when I should have found 5" % (len(reposet),))
    if (not sym) and (len(reposet) != 4):
        print "reposet = %r" % (reposet,)
        raise SystemExit(1, "Found %d repositories when I should have found 4" % (len(reposet),))
    sub1set = frozenset((pjoin('.', 'sub1'),
                         pjoin('.', 'circle', 'subdir', 'sub1')))
    if len(sub1set & reposet) != 1:
        print "sub1set = %r" % (sub1set,)
        print "reposet = %r" % (reposet,)
        raise SystemExit(1, "sub1set and reposet should have exactly one path in common.")
    sub2set = frozenset((pjoin('.', 'subsub1'),
                         pjoin('.', 'subsubdir', 'subsub1')))
    if len(sub2set & reposet) != 1:
        print "sub2set = %r" % (sub2set,)
        print "reposet = %r" % (reposet,)
        raise SystemExit(1, "sub1set and reposet should have exactly one path in common.")
    sub3 = pjoin('.', 'circle', 'top1')
    if sym and not (sub3 in reposet):
        print "reposet = %r" % (reposet,)
        raise SystemExit(1, "Symbolic links are supported and %s is not in reposet" % (sub3,))

runtest()
if sym:
    # Simulate not having symlinks.
    del os.path.samestat
    sym = False
    runtest()
