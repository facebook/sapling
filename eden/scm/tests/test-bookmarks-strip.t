#chg-compatible
#debugruntest-compatible

  $ configure modern
  $ hg init repo
  $ cd repo

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

update to -2 (deactivates the active bookmark)

  $ hg goto -r '.^'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark test2)

  $ echo eee>>qqq.txt

commit new head

  $ hg ci -m 3

bookmarks updated?

  $ hg book
     test                      25e1ee7a0081
     test2                     25e1ee7a0081

strip to revision 1

  $ hg hide 'desc(2)'
  hiding commit 25e1ee7a0081 "2"
  1 changeset hidden
  removing bookmark 'test' (was at: 25e1ee7a0081)
  removing bookmark 'test2' (was at: 25e1ee7a0081)
  2 bookmarks removed

list bookmarks

  $ hg book
  no bookmarks set
