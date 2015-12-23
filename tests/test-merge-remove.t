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
  m   0         -2 unset               bar
  m   0         -2 unset               foo1
  copy: foo -> foo1

  $ hg st -q
  M bar
  M foo1


Removing foo1 and bar:

  $ cp foo1 F
  $ cp bar B
  $ hg rm -f foo1 bar

  $ hg debugstate --nodates
  r   0         -1 set                 bar
  r   0         -1 set                 foo1
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
  n   0         -2 unset               bar
  n   0         -2 unset               foo1
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
  n   0         -2 unset               bar
  n   0         -2 unset               foo1
  copy: foo -> foo1

  $ hg st -qC
  M bar
  M foo1
    foo

  $ hg diff

Merge should not overwrite local file that is untracked after remove

  $ rm *
  $ hg up -qC
  $ hg rm bar
  $ hg ci -m 'remove bar'
  $ echo 'memories of buried pirate treasure' > bar
  $ hg merge
  bar: untracked file differs
  abort: untracked files in working directory differ from files in requested revision
  [255]
  $ cat bar
  memories of buried pirate treasure

Those who use force will lose

  $ hg merge -f
  remote changed bar which local deleted
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved? u
  merging foo1 and foo to foo1
  0 files updated, 1 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ cat bar
  bleh
  $ hg st
  M bar
  M foo1
