  $ hg init
  $ echo a > a
  $ hg commit -A -ma
  adding a

  $ echo b >> a
  $ hg commit -mb

  $ echo c >> a
  $ hg commit -mc

  $ hg up 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo d >> a
  $ hg commit -md
  created new head

  $ hg up 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo e >> a
  $ hg commit -me
  created new head

  $ hg up 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Should fail because not at a head:

  $ hg merge
  abort: branch 'default' has 3 heads - please merge with an explicit rev
  (run 'hg heads .' to see heads)
  [255]

  $ hg up
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Should fail because > 2 heads:

  $ HGMERGE=internal:other; export HGMERGE
  $ hg merge
  abort: branch 'default' has 3 heads - please merge with an explicit rev
  (run 'hg heads .' to see heads)
  [255]

Should succeed:

  $ hg merge 2
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg commit -mm1

Should succeed - 2 heads:

  $ hg merge -P
  changeset:   3:ea9ff125ff88
  parent:      1:1846eede8b68
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     d
  
  $ hg merge
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg commit -mm2

Should fail because at tip:

  $ hg merge
  abort: nothing to merge
  [255]

  $ hg up 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Should fail because there is only one head:

  $ hg merge
  abort: nothing to merge
  (use 'hg update' instead)
  [255]

  $ hg up 3
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ echo f >> a
  $ hg branch foobranch
  marked working directory as branch foobranch
  (branches are permanent and global, did you want a bookmark?)
  $ hg commit -mf

Should fail because merge with other branch:

  $ hg merge
  abort: branch 'foobranch' has one head - please merge with an explicit rev
  (run 'hg heads' to see all heads)
  [255]


Test for issue2043: ensure that 'merge -P' shows ancestors of 6 that
are not ancestors of 7, regardless of where their least common
ancestor is.

Merge preview not affected by common ancestor:

  $ hg up -q 7
  $ hg merge -q -P 6
  2:2d95304fed5d
  4:f25cbe84d8b3
  5:a431fabd6039
  6:e88e33f3bf62

