  $ setconfig extensions.treemanifest=!

#require hardlink

  $ cat > nlinks.py <<EOF
  > from __future__ import print_function
  > import sys
  > from edenscm.mercurial import util
  > for f in sorted(sys.stdin.readlines()):
  >     f = f[:-1]
  >     print(util.nlinks(f), f)
  > EOF

  $ nlinksdir()
  > {
  >     find "$@" -type f | $PYTHON $TESTTMP/nlinks.py
  > }

Some implementations of cp can't create hardlinks (replaces 'cp -al' on Linux):

  $ cat > linkcp.py <<EOF
  > from __future__ import absolute_import
  > import sys
  > from edenscm.mercurial import util
  > util.copyfiles(sys.argv[1], sys.argv[2], hardlink=True)
  > EOF

  $ linkcp()
  > {
  >     $PYTHON $TESTTMP/linkcp.py $1 $2
  > }

Prepare repo r1:

  $ hg init r1
  $ cd r1

  $ echo c1 > f1
  $ hg add f1
  $ hg ci -m0

  $ mkdir d1
  $ cd d1
  $ echo c2 > f2
  $ hg add f2
  $ hg ci -m1
  $ cd ../..

  $ nlinksdir r1/.hg/store
  1 r1/.hg/store/00changelog.i
  1 r1/.hg/store/00manifest.i
  1 r1/.hg/store/data/d1/f2.i
  1 r1/.hg/store/data/f1.i
  1 r1/.hg/store/fncache
  1 r1/.hg/store/phaseroots
  1 r1/.hg/store/requires
  1 r1/.hg/store/undo
  1 r1/.hg/store/undo.backup.fncache
  1 r1/.hg/store/undo.backupfiles
  1 r1/.hg/store/undo.phaseroots


Create hardlinked clone r2:

  $ hg clone -U --debug r1 r2 --config progress.debug=true
  progress: linking: 1
  progress: linking: 2
  progress: linking: 3
  progress: linking: 4
  progress: linking: 5
  progress: linking: 6
  progress: linking: 7
  progress: linking (end)
  linked 7 files

Create non-hardlinked clone r3:

  $ hg clone --pull r1 r3
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  new changesets 40d85e9847f2:7069c422939c
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved


Repos r1 and r2 should now contain hardlinked files:

  $ nlinksdir r1/.hg/store
  2 r1/.hg/store/00changelog.i
  2 r1/.hg/store/00manifest.i
  2 r1/.hg/store/data/d1/f2.i
  2 r1/.hg/store/data/f1.i
  2 r1/.hg/store/fncache
  1 r1/.hg/store/phaseroots
  1 r1/.hg/store/requires
  1 r1/.hg/store/undo
  1 r1/.hg/store/undo.backup.fncache
  1 r1/.hg/store/undo.backupfiles
  1 r1/.hg/store/undo.phaseroots

  $ nlinksdir r2/.hg/store
  2 r2/.hg/store/00changelog.i
  2 r2/.hg/store/00manifest.i
  2 r2/.hg/store/data/d1/f2.i
  2 r2/.hg/store/data/f1.i
  2 r2/.hg/store/fncache

Repo r3 should not be hardlinked:

  $ nlinksdir r3/.hg/store
  1 r3/.hg/store/00changelog.i
  1 r3/.hg/store/00manifest.i
  1 r3/.hg/store/data/d1/f2.i
  1 r3/.hg/store/data/f1.i
  1 r3/.hg/store/fncache
  1 r3/.hg/store/phaseroots
  1 r3/.hg/store/requires
  1 r3/.hg/store/undo
  1 r3/.hg/store/undo.backupfiles
  1 r3/.hg/store/undo.phaseroots


Create a non-inlined filelog in r3:

  $ cd r3/d1
  >>> f = open('data1', 'wb')
  >>> for x in range(10000):
  ...     f.write("%s\n" % str(x))
  >>> f.close()
  $ for j in 0 1 2 3 4 5 6 7 8 9; do
  >   cat data1 >> f2
  >   hg commit -m$j
  > done
  $ cd ../..

  $ nlinksdir r3/.hg/store
  1 r3/.hg/store/00changelog.i
  1 r3/.hg/store/00manifest.i
  1 r3/.hg/store/data/d1/f2.d
  1 r3/.hg/store/data/d1/f2.i
  1 r3/.hg/store/data/f1.i
  1 r3/.hg/store/fncache
  1 r3/.hg/store/phaseroots
  1 r3/.hg/store/requires
  1 r3/.hg/store/undo
  1 r3/.hg/store/undo.backup.fncache
  1 r3/.hg/store/undo.backup.phaseroots
  1 r3/.hg/store/undo.backupfiles
  1 r3/.hg/store/undo.phaseroots

Push to repo r1 should break up most hardlinks in r2:

  $ hg -R r2 verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  2 files, 2 changesets, 2 total revisions

  $ cd r3
  $ hg push
  pushing to $TESTTMP/r1
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 10 changesets with 10 changes to 1 files

  $ cd ..

  $ nlinksdir r2/.hg/store
  1 r2/.hg/store/00changelog.i
  1 r2/.hg/store/00manifest.i
  1 r2/.hg/store/data/d1/f2.i
  2 r2/.hg/store/data/f1.i
  [12] r2/\.hg/store/fncache (re)

#if hardlink-whitelisted
  $ nlinksdir r2/.hg/store/fncache
  2 r2/.hg/store/fncache
#endif

  $ hg -R r2 verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  2 files, 2 changesets, 2 total revisions


  $ cd r1
  $ hg up
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Committing a change to f1 in r1 must break up hardlink f1.i in r2:

  $ echo c1c1 >> f1
  $ hg ci -m00
  $ cd ..

  $ nlinksdir r2/.hg/store
  1 r2/.hg/store/00changelog.i
  1 r2/.hg/store/00manifest.i
  1 r2/.hg/store/data/d1/f2.i
  1 r2/.hg/store/data/f1.i
  [12] r2/\.hg/store/fncache (re)

#if hardlink-whitelisted
  $ nlinksdir r2/.hg/store/fncache
  2 r2/.hg/store/fncache
#endif

Create a file which exec permissions we will change
  $ cd r3
  $ echo "echo hello world" > f3
  $ hg add f3
  $ hg ci -mf3
  $ cd ..

  $ cd r3
  $ hg tip --template '{rev}:{node|short}\n'
  12:d3b77733a28a
  $ echo bla > f1
  $ chmod +x f3
  $ hg ci -m1
  $ cd ..

Create hardlinked copy r4 of r3 (on Linux, we would call 'cp -al'):

  $ linkcp r3 r4

'checklink' is produced by hardlinking a symlink, which is undefined whether
the symlink should be followed or not. It does behave differently on Linux and
BSD. Just remove it so the test pass on both platforms.

  $ rm -f r4/.hg/cache/checklink

r4 has hardlinks in the working dir (not just inside .hg):

  $ nlinksdir r4 | grep -v check
  2 r4/.hg/00changelog.i
  2 r4/.hg/branch
  2 r4/.hg/dirstate
  2 r4/.hg/hgrc
  2 r4/.hg/last-message.txt
  2 r4/.hg/requires
  2 r4/.hg/store/00changelog.i
  2 r4/.hg/store/00manifest.i
  2 r4/.hg/store/data/d1/f2.d
  2 r4/.hg/store/data/d1/f2.i
  2 r4/.hg/store/data/f1.i
  2 r4/.hg/store/data/f3.i
  2 r4/.hg/store/fncache
  2 r4/.hg/store/phaseroots
  2 r4/.hg/store/requires
  2 r4/.hg/store/undo
  2 r4/.hg/store/undo.backup.fncache
  2 r4/.hg/store/undo.backup.phaseroots
  2 r4/.hg/store/undo.backupfiles
  2 r4/.hg/store/undo.phaseroots
  2 r4/.hg/treestate/* (glob)
  [24] r4/\.hg/undo\.backup\.dirstate (re)
  2 r4/.hg/undo.bookmarks
  2 r4/.hg/undo.branch
  2 r4/.hg/undo.desc
  [24] r4/\.hg/undo\.dirstate (re)
  2 r4/d1/data1
  2 r4/d1/f2
  2 r4/f1
  2 r4/f3

Update back to revision 12 in r4 should break hardlink of file f1 and f3:
#if hardlink-whitelisted
  $ nlinksdir r4/.hg/undo.backup.dirstate r4/.hg/undo.dirstate
  4 r4/.hg/undo.backup.dirstate
  4 r4/.hg/undo.dirstate
#endif


  $ hg -R r4 up 12
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved (execbit !)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved (no-execbit !)

  $ nlinksdir r4 | grep -v check
  2 r4/.hg/00changelog.i
  1 r4/.hg/branch
  1 r4/.hg/dirstate
  2 r4/.hg/hgrc
  2 r4/.hg/last-message.txt
  2 r4/.hg/requires
  2 r4/.hg/store/00changelog.i
  2 r4/.hg/store/00manifest.i
  2 r4/.hg/store/data/d1/f2.d
  2 r4/.hg/store/data/d1/f2.i
  2 r4/.hg/store/data/f1.i
  2 r4/.hg/store/data/f3.i
  2 r4/.hg/store/fncache
  2 r4/.hg/store/phaseroots
  2 r4/.hg/store/requires
  2 r4/.hg/store/undo
  2 r4/.hg/store/undo.backup.fncache
  2 r4/.hg/store/undo.backup.phaseroots
  2 r4/.hg/store/undo.backupfiles
  2 r4/.hg/store/undo.phaseroots
  2 r4/.hg/treestate/* (glob)
  [24] r4/\.hg/undo\.backup\.dirstate (re)
  2 r4/.hg/undo.bookmarks
  2 r4/.hg/undo.branch
  2 r4/.hg/undo.desc
  [24] r4/\.hg/undo\.dirstate (re)
  2 r4/d1/data1
  2 r4/d1/f2
  1 r4/f1
  1 r4/f3 (execbit !)
  2 r4/f3 (no-execbit !)

#if hardlink-whitelisted
  $ nlinksdir r4/.hg/undo.backup.dirstate r4/.hg/undo.dirstate
  4 r4/.hg/undo.backup.dirstate
  4 r4/.hg/undo.dirstate
#endif

Test hardlinking outside hg:

  $ mkdir x
  $ echo foo > x/a

  $ linkcp x y
  $ echo bar >> y/a

No diff if hardlink:

  $ diff x/a y/a
