  $ . $TESTDIR/helpers.sh
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "bookmarks=" >> $HGRCPATH
  $ echo "mq=" >> $HGRCPATH

  $ hg init

  $ echo qqq>qqq.txt

add file

  $ hg add
  adding qqq.txt

commit first revision

  $ hg ci -m 1 -u user -d "1 0"

set bookmark

  $ hg book test

  $ echo www>>qqq.txt

commit second revision

  $ hg ci -m 2 -u usr -d "1 0"

set bookmark

  $ hg book test2

update to -2

  $ hg update -r -2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ echo eee>>qqq.txt

commit new head

  $ hg ci -m 3 -u user -d "1 0"
  created new head

bookmarks updated?

  $ hg book
     test                      1:16b24da7e457
     test2                     1:16b24da7e457

strip to revision 1

  $ hg strip 1 | hidebackup
  saved backup bundle to 

list bookmarks

  $ hg book
   * test                      1:9f1b7e78eff8
   * test2                     1:9f1b7e78eff8

