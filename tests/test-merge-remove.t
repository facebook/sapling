  $ hg init

  $ echo foo > foo
  $ echo bar > bar
  $ hg ci -qAm 'add foo bar'

  $ echo foo2 >> foo
  $ echo bleh > bar
  $ hg ci -m 'change foo bar'

  $ hg up -qC 0
  $ hg mv foo foo1
  $ echo foo1 > foo1
  $ hg cat foo >> foo1
  $ hg ci -m 'mv foo foo1'
  created new head

  $ hg merge
  merging foo1 and foo to foo1
  1 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ hg debugstate --nodates
  n   0         -2 bar
  m 644         14 foo1
  copy: foo -> foo1

  $ hg st -q
  M bar
  M foo1


Removing foo1 and bar:

  $ cp foo1 F
  $ cp bar B
  $ hg rm -f foo1 bar

  $ hg debugstate --nodates
  r   0         -2 bar
  r   0         -1 foo1
  copy: foo -> foo1

  $ hg st -qC
  R bar
  R foo1


Re-adding foo1 and bar:

  $ cp F foo1
  $ cp B bar
  $ hg add -v foo1 bar
  adding bar
  adding foo1

  $ hg debugstate --nodates
  n   0         -2 bar
  m 644         14 foo1
  copy: foo -> foo1

  $ hg st -qC
  M bar
  M foo1
    foo


Reverting foo1 and bar:

  $ hg revert -vr . foo1 bar
  saving current version of bar as bar.orig
  reverting bar
  saving current version of foo1 as foo1.orig
  reverting foo1

  $ hg debugstate --nodates
  n   0         -2 bar
  m 644         14 foo1
  copy: foo -> foo1

  $ hg st -qC
  M bar
  M foo1
    foo

  $ hg diff

