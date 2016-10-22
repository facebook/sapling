from __future__ import absolute_import, print_function

import os

from mercurial import (
    hg,
    scmutil,
    ui as uimod,
    util,
)

chdir = os.chdir
mkdir = os.mkdir
pjoin = os.path.join

walkrepos = scmutil.walkrepos
checklink = util.checklink

u = uimod.ui.load()
sym = checklink('.')

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
        print("reposet = %r" % (reposet,))
        print(("Found %d repositories when I should have found 3"
               % (len(reposet),)))
    if (not sym) and (len(reposet) != 2):
        print("reposet = %r" % (reposet,))
        print(("Found %d repositories when I should have found 2"
               % (len(reposet),)))
    sub1set = frozenset((pjoin('.', 'sub1'),
                         pjoin('.', 'circle', 'subdir', 'sub1')))
    if len(sub1set & reposet) != 1:
        print("sub1set = %r" % (sub1set,))
        print("reposet = %r" % (reposet,))
        print("sub1set and reposet should have exactly one path in common.")
    sub2set = frozenset((pjoin('.', 'subsub1'),
                         pjoin('.', 'subsubdir', 'subsub1')))
    if len(sub2set & reposet) != 1:
        print("sub2set = %r" % (sub2set,))
        print("reposet = %r" % (reposet,))
        print("sub2set and reposet should have exactly one path in common.")
    sub3 = pjoin('.', 'circle', 'top1')
    if sym and sub3 not in reposet:
        print("reposet = %r" % (reposet,))
        print("Symbolic links are supported and %s is not in reposet" % (sub3,))

runtest()
if sym:
    # Simulate not having symlinks.
    del os.path.samestat
    sym = False
    runtest()
