#chg-compatible
#debugruntest-compatible

# init

  $ hg init repo
  $ cd repo
  $ echo a > a
  $ hg add a
  $ hg commit -m'a'
  $ echo b > b
  $ hg add b
  $ hg commit -m'b'
  $ hg up -C 'desc(a)'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo c > c
  $ hg add c
  $ hg commit -m'c'

# test merging of diverged bookmarks
  $ hg bookmark -r 'desc(b)' "c@diverge"
  $ hg bookmark -r 'desc(b)' b
  $ hg bookmark c
  $ hg bookmarks
     b                         d2ae7f538514
   * c                         d36c0562f908
     c@diverge                 d2ae7f538514
  $ hg merge "c@diverge"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg commit -m'merge'
  $ hg bookmarks
     b                         d2ae7f538514
   * c                         b8f96cf4688b

  $ hg up -C 'desc(merge)'
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark c)
  $ echo d > d
  $ hg add d
  $ hg commit -m'd'

  $ hg up -C 'desc(merge)'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo e > e
  $ hg add e
  $ hg commit -m'e'
  $ hg up -C 'max(desc(e))'
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bookmark e
  $ hg bookmarks
     b                         d2ae7f538514
     c                         b8f96cf4688b
   * e                         26bee9c5bcf3

# the picked side is bookmarked

  $ hg up -C 'desc(d)'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (leaving bookmark e)
  $ hg merge
  abort: heads are bookmarked - please merge with an explicit rev
  (run 'hg heads' to see all heads)
  [255]

# our revision is bookmarked

  $ hg up -C e
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (activating bookmark e)
  $ hg merge
  abort: no matching bookmark to merge - please merge with an explicit rev or bookmark
  (run 'hg heads' to see all heads)
  [255]

# merge bookmark heads

  $ hg up -C 'desc(d)'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (leaving bookmark e)
  $ echo f > f
  $ hg commit -Am "f"
  adding f
  $ hg bookmarks -r 'desc(d)' "e@diverged"
  $ hg up -q -C "e@diverged"
  $ hg merge
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg parents
  commit:      a0546fcfe0fb
  bookmark:    e@diverged
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     d
  
  commit:      26bee9c5bcf3
  bookmark:    e
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     e
  
  $ hg up -C e
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (activating bookmark e)
  $ hg bookmarks
     b                         d2ae7f538514
     c                         b8f96cf4688b
   * e                         26bee9c5bcf3
     e@diverged                a0546fcfe0fb
  $ hg merge
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg commit -m'merge'
  $ hg bookmarks
     b                         d2ae7f538514
     c                         b8f96cf4688b
   * e                         ca784329f0ba

# test warning when all heads are inactive bookmarks

  $ hg up -C 'desc(f)'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (leaving bookmark e)
  $ echo g > g
  $ hg commit -Am 'g'
  adding g
  $ hg bookmark -i g
  $ hg bookmarks
     b                         d2ae7f538514
     c                         b8f96cf4688b
     e                         ca784329f0ba
     g                         04dd21731d95
  $ hg heads
  commit:      04dd21731d95
  bookmark:    g
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     g
  
  commit:      ca784329f0ba
  bookmark:    e
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     merge
  
  $ hg merge
  abort: heads are bookmarked - please merge with an explicit rev
  (run 'hg heads' to see all heads)
  [255]
