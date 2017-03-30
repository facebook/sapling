  $ $PYTHON -c 'import treemanifest' || exit 80

  $ CACHEDIR=$PWD/hgcache
  $ . "$TESTDIR/library.sh"


Test that treemanifest backfill populates the database

  $ initserver master master
  $ initserver master-alreadysynced master
  $ initserver master-new master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=
  > [treemanifest]
  > server = True
  > EOF
  $ touch a && hg ci -Aqm a
  $ mkdir dir
  $ touch dir/b && hg ci -Aqm b

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
  $ hg backfilltree
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
