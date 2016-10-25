  $ hg init

Revision 0:

  $ echo "unchanged" > unchanged
  $ echo "remove me" > remove
  $ echo "copy me" > copy
  $ echo "move me" > move
  $ for i in 1 2 3 4 5 6 7 8 9; do
  >     echo "merge ok $i" >> zzz1_merge_ok
  > done
  $ echo "merge bad" > zzz2_merge_bad
  $ hg ci -Am "revision 0"
  adding copy
  adding move
  adding remove
  adding unchanged
  adding zzz1_merge_ok
  adding zzz2_merge_bad

Revision 1:

  $ hg rm remove
  $ hg mv move moved
  $ hg cp copy copied
  $ echo "added" > added
  $ hg add added
  $ echo "new first line" > zzz1_merge_ok
  $ hg cat zzz1_merge_ok >> zzz1_merge_ok
  $ echo "new last line" >> zzz2_merge_bad
  $ hg ci -m "revision 1"

Local changes to revision 0:

  $ hg co 0
  4 files updated, 0 files merged, 3 files removed, 0 files unresolved
  $ echo "new last line" >> zzz1_merge_ok
  $ echo "another last line" >> zzz2_merge_bad

  $ hg diff --nodates | grep "^[+-][^<>]"
  --- a/zzz1_merge_ok
  +++ b/zzz1_merge_ok
  +new last line
  --- a/zzz2_merge_bad
  +++ b/zzz2_merge_bad
  +another last line

  $ hg st
  M zzz1_merge_ok
  M zzz2_merge_bad

Local merge with bad merge tool:

  $ HGMERGE=false hg co
  merging zzz1_merge_ok
  merging zzz2_merge_bad
  merging zzz2_merge_bad failed!
  3 files updated, 1 files merged, 2 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges
  [1]

  $ hg resolve -m
  (no more unresolved files)

  $ hg co 0
  merging zzz1_merge_ok
  merging zzz2_merge_bad
  warning: conflicts while merging zzz2_merge_bad! (edit, then use 'hg resolve --mark')
  2 files updated, 1 files merged, 3 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges
  [1]

  $ hg diff --nodates | grep "^[+-][^<>]"
  --- a/zzz1_merge_ok
  +++ b/zzz1_merge_ok
  +new last line
  --- a/zzz2_merge_bad
  +++ b/zzz2_merge_bad
  +another last line
  +=======

  $ hg st
  M zzz1_merge_ok
  M zzz2_merge_bad
  ? zzz2_merge_bad.orig

Local merge with conflicts:

  $ hg resolve -m
  (no more unresolved files)

  $ hg co
  merging zzz1_merge_ok
  merging zzz2_merge_bad
  warning: conflicts while merging zzz2_merge_bad! (edit, then use 'hg resolve --mark')
  3 files updated, 1 files merged, 2 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges
  [1]

  $ hg resolve -m
  (no more unresolved files)

  $ hg co 0 --config 'ui.origbackuppath=.hg/origbackups'
  merging zzz1_merge_ok
  merging zzz2_merge_bad
  warning: conflicts while merging zzz2_merge_bad! (edit, then use 'hg resolve --mark')
  2 files updated, 1 files merged, 3 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges
  [1]

Are orig files from the last commit where we want them?
  $ ls .hg/origbackups
  zzz2_merge_bad.orig

  $ hg diff --nodates | grep "^[+-][^<>]"
  --- a/zzz1_merge_ok
  +++ b/zzz1_merge_ok
  +new last line
  --- a/zzz2_merge_bad
  +++ b/zzz2_merge_bad
  +another last line
  +=======
  +=======
  +new last line
  +=======

  $ hg st
  M zzz1_merge_ok
  M zzz2_merge_bad
  ? zzz2_merge_bad.orig

Local merge without conflicts:

  $ hg revert zzz2_merge_bad

  $ hg resolve -m
  (no more unresolved files)

  $ hg co
  merging zzz1_merge_ok
  4 files updated, 1 files merged, 2 files removed, 0 files unresolved

  $ hg diff --nodates | grep "^[+-][^<>]"
  --- a/zzz1_merge_ok
  +++ b/zzz1_merge_ok
  +new last line

  $ hg st
  M zzz1_merge_ok
  ? zzz2_merge_bad.orig

