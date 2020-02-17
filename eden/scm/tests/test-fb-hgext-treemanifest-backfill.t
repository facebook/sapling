#chg-compatible

  $ disable treemanifest
  $ CACHEDIR=`pwd`/hgcache
  $ setconfig treemanifest.treeonly=False

  $ hg init master
  $ cd master
  $ echo x > x
  $ hg commit -qAm 'add x'
  $ mkdir subdir
  $ echo z > subdir/z
  $ hg commit -qAm 'add subdir/z'
  $ echo x >> x
  $ hg commit -qAm 'modify x'
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=
  > 
  > [remotefilelog]
  > name=master
  > cachepath=$CACHEDIR
  > 
  > [fastmanifest]
  > usetree=True
  > usecache=False
  > 
  > [treemanifest]
  > server=True
  > EOF
  $ hg backfilltree -l 1 --traceback
  $ ls .hg/store/00m*
  .hg/store/00manifest.i
  .hg/store/00manifesttree.i
  $ hg debugindex .hg/store/00manifesttree.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      44     -1       0 bc0c2c938b92 000000000000 000000000000
  $ hg debugindex .hg/store/00manifest.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      44     -1       0 bc0c2c938b92 000000000000 000000000000
       1        44      62      0       1 70f2c6726cec bc0c2c938b92 000000000000
       2       106      55      1       2 68221fc1644f 70f2c6726cec 000000000000

  $ hg backfilltree -l 1 --traceback
  $ ls .hg/store/00m*
  .hg/store/00manifest.i
  .hg/store/00manifesttree.i
  $ ls .hg/store/meta
  subdir
  $ hg debugindex .hg/store/00manifesttree.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      44     -1       0 bc0c2c938b92 000000000000 000000000000
       1        44      61      0       1 70f2c6726cec bc0c2c938b92 000000000000
  $ hg debugindex .hg/store/00manifest.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      44     -1       0 bc0c2c938b92 000000000000 000000000000
       1        44      62      0       1 70f2c6726cec bc0c2c938b92 000000000000
       2       106      55      1       2 68221fc1644f 70f2c6726cec 000000000000

  $ hg debugdata .hg/store/00manifesttree.i 0
  x\x001406e74118627694268417491f018a4a883152f0 (esc)
  $ hg debugdata .hg/store/00manifesttree.i 1
  subdir\x00ddb35f099a648a43a997aef53123bce309c794fdt (esc)
  x\x001406e74118627694268417491f018a4a883152f0 (esc)

Test backfilling all at once
  $ rm -rf .hg/store/00manifesttree.i .hg/store/meta
  $ hg backfilltree
  $ hg debugindex .hg/store/00manifesttree.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      44     -1       0 bc0c2c938b92 000000000000 000000000000
       1        44      61      0       1 70f2c6726cec bc0c2c938b92 000000000000
       2       105      55      1       2 68221fc1644f 70f2c6726cec 000000000000

Test backfilling a commit with a null manifest
  $ cd ../
  $ hg init nullrepo
  $ cd nullrepo
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=
  > 
  > [remotefilelog]
  > name=master
  > cachepath=$CACHEDIR
  > 
  > [fastmanifest]
  > usetree=True
  > usecache=False
  > 
  > [treemanifest]
  > server=True
  > EOF
  $ hg commit --config ui.allowemptycommit=True -m "Initial commit"
  $ hg backfilltree
