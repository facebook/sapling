#chg-compatible
#debugruntest-compatible

  $ configure mutation-norecord
  $ enable rebase

initialize repository

  $ hg init repo
  $ cd repo

  $ echo 'a' > a
  $ hg ci -A -m "0"
  adding a

  $ echo 'b' > b
  $ hg ci -A -m "1"
  adding b

  $ hg up 'desc(0)'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo 'c' > c
  $ hg ci -A -m "2"
  adding c

  $ echo 'd' > d
  $ hg ci -A -m "3"
  adding d

  $ hg bookmark -r 'desc(1)' one
  $ hg bookmark -r 'desc(3)' two
  $ hg up -q two

bookmark list

  $ hg bookmark
     one                       925d80f479bb
   * two                       2ae46b1d99a7

rebase

  $ hg rebase -s two -d one
  rebasing 2ae46b1d99a7 "3" (two)

  $ hg log
  commit:      42e5ed2cdcf4
  bookmark:    two
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     3
  
  commit:      db815d6d32e6
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2
  
  commit:      925d80f479bb
  bookmark:    one
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1
  
  commit:      f7b1eb17ad24
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     0
  
aborted rebase should restore active bookmark.

  $ hg up 'desc(1)'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (leaving bookmark two)
  $ echo 'e' > d
  $ hg ci -A -m "4"
  adding d
  $ hg bookmark three
  $ hg rebase -s three -d two
  rebasing dd7c838e8362 "4" (three)
  merging d
  warning: 1 conflicts while merging d! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ hg rebase --abort
  rebase aborted
  $ hg bookmark
     one                       925d80f479bb
   * three                     dd7c838e8362
     two                       42e5ed2cdcf4

after aborted rebase, restoring a bookmark that has been removed should not fail

  $ hg rebase -s three -d two
  rebasing dd7c838e8362 "4" (three)
  merging d
  warning: 1 conflicts while merging d! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ hg bookmark -d three
  $ hg rebase --abort
  rebase aborted
  $ hg bookmark
     one                       925d80f479bb
     two                       42e5ed2cdcf4
