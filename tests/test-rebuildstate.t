basic test for hg debugrebuildstate

  $ hg init repo
  $ cd repo

  $ touch foo bar
  $ hg ci -Am 'add foo bar'
  adding bar
  adding foo

  $ touch baz
  $ hg add baz
  $ hg rm bar

  $ hg debugrebuildstate

state dump after

  $ hg debugstate --nodates | sort
  n 644         -1 bar
  n 644         -1 foo

status

  $ hg st -A
  ! bar
  ? baz
  C foo

  $ cd ..
