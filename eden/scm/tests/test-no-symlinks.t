#require no-symlink
  $ disable treemanifest
  $ setconfig workingcopy.ruststatus=False

# The following script was used to create the bundle:
#
# hg init symlinks
# cd symlinks
# echo a > a
# mkdir d
# echo b > d/b
# ln -s a a.lnk
# ln -s d/b d/b.lnk
# hg ci -Am t
# hg bundle --base null ../test-no-symlinks.hg

Extract a symlink on a platform not supporting them

  $ hg init t
  $ cd t
  $ hg pull -q "$TESTDIR/bundles/test-no-symlinks.hg"
  $ hg goto
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat a.lnk && echo
  a
  $ cat d/b.lnk && echo
  d/b

Copy a symlink and move another

  $ hg copy a.lnk d/a2.lnk
  $ hg mv d/b.lnk b2.lnk
  $ hg ci -Am copy
  $ cat d/a2.lnk && echo
  a
  $ cat b2.lnk && echo
  d/b

Bundle and extract again

  $ hg bundle --base null ../symlinks.hg
  2 changesets found
  $ cd ..
  $ hg init t2
  $ cd t2
  $ hg pull ../symlinks.hg
  pulling from ../symlinks.hg
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  $ hg goto
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat a.lnk && echo
  a
  $ cat d/a2.lnk && echo
  a
  $ cat b2.lnk && echo
  d/b
