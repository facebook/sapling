#chg-compatible
#debugruntest-compatible

Test that checks that relative paths are used in merge

  $ unset HGMERGE # make sure HGMERGE doesn't interfere with the test
  $ hg init repo
  $ cd repo

  $ mkdir dir && echo a > dir/file
  $ hg ci -Aqm first

  $ hg up -q null
  $ mkdir dir && echo b > dir/file
  $ hg ci -Aqm second

  $ hg up -q 'desc(first)'

  $ hg merge 'desc(second)'
  merging dir/file
  warning: 1 conflicts while merging dir/file! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]

  $ hg up -q -C .
  $ cd dir
  $ hg merge 'desc(second)'
  merging file
  warning: 1 conflicts while merging file! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]

Merging with different paths
  $ cd ..
  $ rm -rf repo
  $ hg init repo
  $ cd repo
  $ mkdir dir && echo a > dir/file
  $ hg ci -Aqm common
  $ echo b > dir/file
  $ hg commit -Am modify

  $ hg up -q 'desc(common)'
  $ mkdir dir2
  $ hg mv dir/file dir2/file
  $ hg ci -Aqm move
  $ hg merge 'desc(modify)'
  merging dir2/file and dir/file to dir2/file
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg up -q -C .
  $ cd dir2
  $ hg merge 'desc(modify)'
  merging file and ../dir/file to file
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

