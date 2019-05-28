  $ setconfig extensions.treemanifest=!
Set up global extensions
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > rebase=
  > EOF

Create a repo without remotenames
  $ hg init off
  $ cd off
  $ echo a > a
  $ hg ci -qAm a
  $ cd ..

Clone repo and turn remotenames on
  $ hg clone off on
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat >> on/.hg/hgrc <<EOF
  > [extensions]
  > remotenames=
  > EOF

Ensure no crashes when working from repo with remotenames on
  $ hg -R off bookmark foo
  $ cd on

  $ hg pull
  pulling from $TESTTMP/off
  searching for changes
  no changes found

  $ hg push --to bar --create
  pushing rev cb9a9f314b8b to destination $TESTTMP/off bookmark bar
  searching for changes
  no changes found
  exporting bookmark bar
  [1]

  $ hg pull --rebase
  pulling from $TESTTMP/off
  searching for changes
  no changes found

  $ cd ..

Check for crashes when working from repo with remotenames off
  $ cd off

  $ hg pull ../on
  pulling from ../on
  searching for changes
  no changes found

  $ cat >> .hg/hgrc <<EOF
  > [paths]
  > default = $TESTTMP/on
  > EOF

  $ hg pull
  pulling from $TESTTMP/on
  searching for changes
  no changes found

  $ hg push
  pushing to $TESTTMP/on
  searching for changes
  no changes found
  [1]

  $ hg pull --rebase
  pulling from $TESTTMP/on
  searching for changes
  no changes found
