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

  $ hg pull ../on 2>&1 | grep Error
  AttributeError: 'localrepository' object has no attribute '_remotenames'

  $ cat >> .hg/hgrc <<EOF
  > [paths]
  > default = $TESTTMP/on
  > EOF

  $ hg pull 2>&1 | grep Error
  AttributeError: 'localrepository' object has no attribute '_remotenames'

  $ hg push 2>&1 | grep Error
  AttributeError: 'localrepository' object has no attribute '_remotenames'

  $ hg pull --rebase 2>&1 | grep Error
  AttributeError: 'localrepository' object has no attribute '_remotenames'
