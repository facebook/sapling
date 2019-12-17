#chg-compatible

  $ . helpers-usechg.sh

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

  $ hg up 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo e >> a
  $ hg commit -me

  $ hg up 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Should fail because not at a head:

  $ hg merge
  abort: working directory not at a head revision
  (use 'hg update' or merge with an explicit revision)
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

  $ hg merge 2
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg id -Tjson
  [
   {
    "bookmarks": [],
    "dirty": "+",
    "id": "f25cbe84d8b3+2d95304fed5d+",
    "node": "ffffffffffffffffffffffffffffffffffffffff",
    "parents": [{"node": "f25cbe84d8b320e298e7703f18a25a3959518c23", "rev": 4}, {"node": "2d95304fed5d89bc9d70b2a0d02f0d567469c3ab", "rev": 2}],
    "tags": []
   }
  ]
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

  $ hg id -r 1 -Tjson
  [
   {
    "bookmarks": [],
    "id": "1846eede8b68",
    "node": "1846eede8b6886d8cc8a88c96a687b7fe8f3b9d1",
    "tags": []
   }
  ]

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

Test for issue2043: ensure that 'merge -P' shows ancestors of 6 that
are not ancestors of 7, regardless of where their common ancestors are.

Merge preview not affected by common ancestor:

  $ hg merge -q -P 6
  2:2d95304fed5d
  4:f25cbe84d8b3
  5:a431fabd6039
  6:e88e33f3bf62

Test experimental destination revset

  $ hg log -r '_destmerge()'
  abort: nothing to merge
  (use 'hg update' instead)
  [255]

(on a branch with a two heads)

  $ hg up 5
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo f >> a
  $ hg commit -mf
  $ hg log -r '_destmerge()'
  changeset:   6:e88e33f3bf62
  parent:      5:a431fabd6039
  parent:      3:ea9ff125ff88
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     m2
  

(from the other head)

  $ hg log -r '_destmerge(e88e33f3bf62)'
  changeset:   7:b613918999e2
  parent:      5:a431fabd6039
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     f
  

