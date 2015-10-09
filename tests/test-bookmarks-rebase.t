  $ echo "[extensions]" >> $HGRCPATH
  $ echo "rebase=" >> $HGRCPATH

initialize repository

  $ hg init

  $ echo 'a' > a
  $ hg ci -A -m "0"
  adding a

  $ echo 'b' > b
  $ hg ci -A -m "1"
  adding b

  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo 'c' > c
  $ hg ci -A -m "2"
  adding c
  created new head

  $ echo 'd' > d
  $ hg ci -A -m "3"
  adding d

  $ hg bookmark -r 1 one
  $ hg bookmark -r 3 two
  $ hg up -q two

bookmark list

  $ hg bookmark
     one                       1:925d80f479bb
   * two                       3:2ae46b1d99a7

rebase

  $ hg rebase -s two -d one
  rebasing 3:2ae46b1d99a7 "3" (tip two)
  saved backup bundle to $TESTTMP/.hg/strip-backup/2ae46b1d99a7-e6b057bc-backup.hg (glob)

  $ hg log
  changeset:   3:42e5ed2cdcf4
  bookmark:    two
  tag:         tip
  parent:      1:925d80f479bb
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     3
  
  changeset:   2:db815d6d32e6
  parent:      0:f7b1eb17ad24
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2
  
  changeset:   1:925d80f479bb
  bookmark:    one
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1
  
  changeset:   0:f7b1eb17ad24
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     0
  
aborted rebase should restore active bookmark.

  $ hg up 1
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (leaving bookmark two)
  $ echo 'e' > d
  $ hg ci -A -m "4"
  adding d
  created new head
  $ hg bookmark three
  $ hg rebase -s three -d two
  rebasing 4:dd7c838e8362 "4" (tip three)
  merging d
  warning: conflicts while merging d! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ hg rebase --abort
  rebase aborted
  $ hg bookmark
     one                       1:925d80f479bb
   * three                     4:dd7c838e8362
     two                       3:42e5ed2cdcf4

after aborted rebase, restoring a bookmark that has been removed should not fail

  $ hg rebase -s three -d two
  rebasing 4:dd7c838e8362 "4" (tip three)
  merging d
  warning: conflicts while merging d! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ hg bookmark -d three
  $ hg rebase --abort
  rebase aborted
  $ hg bookmark
     one                       1:925d80f479bb
     two                       3:42e5ed2cdcf4
