  $ echo "[extensions]" >> $HGRCPATH
  $ echo "bookmarks=" >> $HGRCPATH
  $ echo "mq=" >> $HGRCPATH

  $ hg init

  $ echo qqq>qqq.txt

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

update to -2

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
  saved backup bundle to .*

list bookmarks

  $ hg book
   * test                      1:8cf31af87a2b
   * test2                     1:8cf31af87a2b

