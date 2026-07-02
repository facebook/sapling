#modern-config-incompatible

#require no-eden no-symlink

TODO(debugruntest): this test fails on Mac


# The following (test) script was used to create the bundle:
#
# configure modernclient
# newclientrepo symlinks
# echo a > a
# mkdir d
# echo b > d/b
# ln -s a a.lnk
# ln -s d/b d/b.lnk
# sl ci -Am t
# sl bundle --base null $TESTDIR/bundles/test-no-symlinks.hg

Extract a symlink on a platform not supporting them

  $ sl init t
  $ cd t
  $ sl unbundle -q "$TESTDIR/bundles/test-no-symlinks.hg"
  $ sl goto tip
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat a.lnk && echo
  a
  $ cat d/b.lnk && echo
  d/b

Copy a symlink and move another

  $ sl copy a.lnk d/a2.lnk
  $ sl mv d/b.lnk b2.lnk
  $ sl ci -Am copy
  $ cat d/a2.lnk && echo
  a
  $ cat b2.lnk && echo
  d/b

Bundle and extract again

  $ sl bundle --base null ../symlinks.hg
  2 changesets found
  $ cd ..
  $ sl init t2
  $ cd t2
  $ sl unbundle -q ../symlinks.hg
  $ sl goto tip
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat a.lnk && echo
  a
  $ cat d/a2.lnk && echo
  a
  $ cat b2.lnk && echo
  d/b
