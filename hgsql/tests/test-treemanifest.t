  $ $PYTHON -c 'import treemanifest' || exit 80

  $ CACHEDIR=`pwd`/hgcache
  $ . "$TESTDIR/library.sh"

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > pushrebase=
  > EOF

Test that treemanifest backfill populates the database

  $ initserver master master
  $ initserver master-alreadysynced master
  $ initserver master-new master
  $ cd master
  $ touch a && hg ci -Aqm a
  $ mkdir dir
  $ touch dir/b && hg ci -Aqm b
  $ hg book master

  $ cd ../master-alreadysynced
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=
  > [treemanifest]
  > server = True
  > EOF
  $ hg log -r tip --forcesync -T '{rev}\n'
  1

  $ cd ../master
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=
  > [treemanifest]
  > server = True
  > EOF
  $ DBGD=1 hg backfilltree
  $ ls .hg/store/meta/dir
  00manifest.i

Test that an empty repo syncs the tree revlogs

  $ cd ../master-new
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=
  > [treemanifest]
  > server = True
  > EOF
  $ hg log -r tip --forcesync -T '{rev}\n'
  1
  $ ls .hg/store/meta/dir
  00manifest.i

Test that we can replay backfills into an existing repo
  $ cd ../master-alreadysynced
  $ hg sqlreplay
  $ ls .hg/store/meta/dir
  00manifest.i
  $ rm -rf .hg/store/00manifesttree* .hg/store/meta
  $ hg sqlreplay --start 0 --end 0
  $ hg debugindex .hg/store/00manifesttree.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      44     -1       0 8515d4bfda76 000000000000 000000000000
  $ hg sqlreplay --start 1 --end 2
  $ hg debugindex .hg/store/00manifesttree.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      44     -1       0 8515d4bfda76 000000000000 000000000000
       1        44      58      0       1 898d94054864 8515d4bfda76 000000000000
  $ cd ..

Test that trees created during push are synced to the db

  $ initclient client
  $ cd client
  $ hg pull -q ssh://user@dummy/master
  $ hg up -q tip
  $ touch dir/c && hg ci -Aqm c

  $ hg push ssh://user@dummy/master --to master
  pushing to ssh://user@dummy/master
  searching for changes
  remote: pushing 1 changeset:
  remote:     c46827e4453c  c

  $ cd ../master-new
  $ hg log -G -T '{rev} {desc}' --forcesync
  o  2 c
  |
  o  1 b
  |
  o  0 a
  
  $ hg debugdata .hg/store/meta/dir/00manifest.i 1
  b\x00b80de5d138758541c5f05265ad144ab9fa86d1db (esc)
  c\x00b80de5d138758541c5f05265ad144ab9fa86d1db (esc)
