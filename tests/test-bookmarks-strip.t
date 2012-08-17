  $ echo "[extensions]" >> $HGRCPATH
  $ echo "mq=" >> $HGRCPATH

  $ hg init

  $ echo qqq>qqq.txt

rollback dry run without rollback information

  $ hg rollback
  no rollback information available
  [1]

add file

  $ hg add
  adding qqq.txt

commit first revision

  $ hg ci -m 1

set bookmark

  $ hg book test

  $ echo www>>qqq.txt

commit second revision

  $ hg ci -m 2

set bookmark

  $ hg book test2

update to -2 (deactivates the active bookmark)

  $ hg update -r -2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ echo eee>>qqq.txt

commit new head

  $ hg ci -m 3
  created new head

bookmarks updated?

  $ hg book
     test                      1:25e1ee7a0081
     test2                     1:25e1ee7a0081

strip to revision 1

  $ hg strip 1
  saved backup bundle to $TESTTMP/.hg/strip-backup/*-backup.hg (glob)

list bookmarks

  $ hg book
     test                      0:5c9ad3787638
     test2                     0:5c9ad3787638

immediate rollback and reentrancy issue

  $ echo "mq=!" >> $HGRCPATH
  $ hg init repo
  $ cd repo
  $ echo a > a
  $ hg ci -Am adda
  adding a
  $ echo b > b
  $ hg ci -Am addb
  adding b
  $ hg bookmarks markb
  $ hg rollback
  repository tip rolled back to revision 0 (undo commit)
  working directory now based on revision 0

are you there?

  $ hg bookmarks
  no bookmarks set

can we commit? (issue2692)

  $ echo c > c
  $ hg ci -Am rockon
  adding c

can you be added again?

  $ hg bookmarks markb
  $ hg bookmarks
   * markb                     1:fdb34407462c

rollback dry run with rollback information

  $ hg rollback -n
  repository tip rolled back to revision 0 (undo commit)
  $ hg bookmarks
   * markb                     1:fdb34407462c

rollback dry run with rollback information and no commit undo

  $ rm .hg/store/undo
  $ hg rollback -n
  no rollback information available
  [1]
  $ hg bookmarks
   * markb                     1:fdb34407462c

  $ cd ..

