  $ $PYTHON -c 'import treemanifest' || exit 80

  $ CACHEDIR=$PWD/hgcache
  $ . "$TESTDIR/library.sh"

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > bundle2hooks=
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
  remote: pushing 1 changset:
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
