
  $ . "$TESTDIR/library.sh"

  $ hginit master
  $ hginit sane

  $ enable treemanifest
  $ setconfig treemanifest.treeonly=True

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

  $ hgcloneshallow ssh://user@dummy/sane shallow -q --config extensions.fastmanifest=
  fetching tree '' bc0c2c938b929f98b1c31a8c5994396ebb096bf0
  1 trees fetched over *s (glob)
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)

Verifies that both repositories will be garbage collected

  $ hg gc master sane
  finished: removed 0 of 1 files (0.00 GB to 0.00 GB)
  warning: can't gc repository: $TESTTMP/master: "unable to find the following nodes locally or on the server: ('', bc0c2c938b929f98b1c31a8c5994396ebb096bf0)"
