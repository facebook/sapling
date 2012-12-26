# init

  $ hg init
  $ echo a > a
  $ hg add a
  $ hg commit -m'a'
  $ echo b > b
  $ hg add b
  $ hg commit -m'b'
  $ hg up -C 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo c > c
  $ hg add c
  $ hg commit -m'c'
  created new head

# test merging of diverged bookmarks
  $ hg bookmark -r 1 "c@diverge"
  $ hg bookmark -r 1 b
  $ hg bookmark c
  $ hg bookmarks
     b                         1:d2ae7f538514
   * c                         2:d36c0562f908
     c@diverge                 1:d2ae7f538514
  $ hg merge "c@diverge"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg commit -m'merge'
  $ hg bookmarks
     b                         1:d2ae7f538514
   * c                         3:b8f96cf4688b

  $ hg up -C 3
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo d > d
  $ hg add d
  $ hg commit -m'd'

  $ hg up -C 3
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo e > e
  $ hg add e
  $ hg commit -m'e'
  created new head
  $ hg up -C 5
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bookmark e
  $ hg bookmarks
     b                         1:d2ae7f538514
     c                         3:b8f96cf4688b
   * e                         5:26bee9c5bcf3

# the picked side is bookmarked

  $ hg up -C 4
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg merge
  abort: heads are bookmarked - please merge with an explicit rev
  (run 'hg heads' to see all heads)
  [255]

# our revision is bookmarked

  $ hg up -C e
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg merge
  abort: no matching bookmark to merge - please merge with an explicit rev or bookmark
  (run 'hg heads' to see all heads)
  [255]

# merge bookmark heads

  $ hg up -C 4
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo f > f
  $ hg commit -Am "f"
  adding f
  $ hg bookmarks -r 4 "e@diverged"
  $ hg up -q -C "e@diverged"
  $ hg merge
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg parents
  changeset:   4:a0546fcfe0fb
  bookmark:    e@diverged
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     d
  
  changeset:   5:26bee9c5bcf3
  bookmark:    e
  parent:      3:b8f96cf4688b
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     e
  
  $ hg up -C e
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg bookmarks
     b                         1:d2ae7f538514
     c                         3:b8f96cf4688b
   * e                         5:26bee9c5bcf3
     e@diverged                4:a0546fcfe0fb
  $ hg merge
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg commit -m'merge'
  $ hg bookmarks
     b                         1:d2ae7f538514
     c                         3:b8f96cf4688b
   * e                         7:ca784329f0ba

# test warning when all heads are inactive bookmarks

  $ hg up -C 6
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo g > g
  $ hg commit -Am 'g'
  adding g
  $ hg bookmark -i g
  $ hg bookmarks
     b                         1:d2ae7f538514
     c                         3:b8f96cf4688b
     e                         7:ca784329f0ba
     g                         8:04dd21731d95
  $ hg heads
  changeset:   8:04dd21731d95
  bookmark:    g
  tag:         tip
  parent:      6:be381d1126a0
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     g
  
  changeset:   7:ca784329f0ba
  bookmark:    e
  parent:      5:26bee9c5bcf3
  parent:      4:a0546fcfe0fb
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     merge
  
  $ hg merge
  abort: heads are bookmarked - please merge with an explicit rev
  (run 'hg heads' to see all heads)
  [255]
