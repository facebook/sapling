#chg-compatible


  $ . "$TESTDIR/library.sh"

  $ hginit master
  $ hginit sane

  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > server=True
  > [treemanifest]
  > server=True
  > treeonly=True
  > EOF
  $ echo x > x
  $ hg commit -qAm x1
  $ rm -f .hg/store/00manifesttree.i
  $ cd ..

Populate the second repository

  $ cd sane
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > [remotefilelog]
  > server=True
  > [treemanifest]
  > server=True
  > treeonly=True
  > EOF
  $ echo x > x
  $ hg commit -qAm x1
  $ cd ..

  $ hgcloneshallow ssh://user@dummy/sane shallow -q
  fetching tree '' bc0c2c938b929f98b1c31a8c5994396ebb096bf0
  1 trees fetched over *s (glob)
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)
