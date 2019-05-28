  $ setconfig extensions.treemanifest=!

  $ . "$TESTDIR/library.sh"
  $ setconfig treemanifest.treeonly=False

  $ hginit master

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > treemanifest=
  > EOF

  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > [remotefilelog]
  > server=True
  > [treemanifest]
  > server=True
  > EOF
  $ mkdir dir
  $ echo x > dir/x
  $ hg commit -qAm x1
  $ hg backfilltree
  $ cd ..

Clone with shallowtrees not set (False)

  $ hgcloneshallow ssh://user@dummy/master shallow --noupdate --config extensions.fastmanifest=
  streaming all changes
  3 files to transfer, 347 bytes of data
  transferred 347 bytes in * seconds (*/sec) (glob)
  searching for changes
  no changes found
  $ ls shallow/.hg/store/00*.i
  shallow/.hg/store/00changelog.i
  shallow/.hg/store/00manifest.i
  shallow/.hg/store/00manifesttree.i
  $ rm -rf shallow

Clone with shallowtrees=True
  $ cat >> master/.hg/hgrc <<EOF
  > [remotefilelog]
  > shallowtrees=True
  > EOF

  $ hgcloneshallow ssh://user@dummy/master shallow --noupdate --config extensions.fastmanifest=
  streaming all changes
  2 files to transfer, 236 bytes of data
  transferred 236 bytes in * seconds (*/sec) (glob)
  searching for changes
  no changes found
  $ ls shallow/.hg/store/00*.i
  shallow/.hg/store/00changelog.i
  shallow/.hg/store/00manifest.i
