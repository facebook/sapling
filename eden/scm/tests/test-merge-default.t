#chg-compatible
#debugruntest-compatible


  $ setconfig devel.segmented-changelog-rev-compat=true
  $ hg init repo
  $ cd repo
  $ echo a > a
  $ hg commit -A -ma
  adding a

  $ echo b >> a
  $ hg commit -mb

  $ echo c >> a
  $ hg commit -mc

  $ hg up 'desc(b)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo d >> a
  $ hg commit -md

  $ hg up 'desc(b)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo e >> a
  $ hg commit -me

  $ hg up 'desc(b)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Should fail because not at a head:

  $ hg merge
  abort: working directory not at a head revision
  (use 'hg goto' or merge with an explicit revision)
  [255]

  $ hg up
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated to "f25cbe84d8b3: e"
  2 other heads for branch "default"

Should fail because > 2 heads:

  $ HGMERGE=internal:other; export HGMERGE
  $ hg merge
  abort: branch 'default' has 3 heads - please merge with an explicit rev
  (run 'hg heads .' to see heads)
  [255]

Should succeed:

  $ hg merge 'desc(c)'
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg id -Tjson
  [
   {
    "bookmarks": [],
    "dirty": "+",
    "id": "f25cbe84d8b3+2d95304fed5d+",
    "node": "ffffffffffffffffffffffffffffffffffffffff",
    "parents": [{"node": "f25cbe84d8b320e298e7703f18a25a3959518c23", "rev": 4}, {"node": "2d95304fed5d89bc9d70b2a0d02f0d567469c3ab", "rev": 2}]
   }
  ]
  $ hg commit -mm1

Should succeed - 2 heads:

  $ hg merge -P
  commit:      ea9ff125ff88
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     d
  
  $ hg merge
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg commit -mm2

  $ hg id -r 'desc(b)' -Tjson
  [
   {
    "bookmarks": [],
    "id": "1846eede8b68",
    "node": "1846eede8b6886d8cc8a88c96a687b7fe8f3b9d1"
   }
  ]

Should fail because at tip:

  $ hg merge
  abort: nothing to merge
  [255]

  $ hg up 'desc(a)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Should fail because there is only one head:

  $ hg merge
  abort: nothing to merge
  (use 'hg goto' instead)
  [255]

  $ hg up 'desc(d)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Test for issue2043: ensure that 'merge -P' shows ancestors of 6 that
are not ancestors of 7, regardless of where their common ancestors are.

Merge preview not affected by common ancestor:

  $ hg merge -q -P 'desc(m2)'
  2d95304fed5d
  f25cbe84d8b3
  a431fabd6039
  e88e33f3bf62
