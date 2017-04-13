====================================
Testing head checking code: Case A-3
====================================

Mercurial checks for the introduction of new heads on push. Evolution comes
into play to detect if existing branches on the server are being replaced by
some of the new one we push.

This case is part of a series of tests checking this behavior.

Category A: simple case involving a branch being superceeded by another.
TestCase 3: multi-changeset branch with reordering

Push should be allowed
.. old-state:
..
.. * 2 changeset branch
..
.. new-state:
..
.. * 2 changeset branch succeeding the old one with reordering
..
.. expected-result:
..
.. * push allowed
..
.. graph-summary:
..
..   B ø⇠⇠
..     | ⇡
..   A ø⇠⇠⇠○ A'
..     | ⇡/
..     | ○ B'
..     |/
..     ●

  $ . $TESTDIR/testlib/push-checkheads-util.sh

Test setup
----------

  $ mkdir A3
  $ cd A3
  $ setuprepos
  creating basic server and client repo
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd server
  $ mkcommit B0
  $ cd ../client
  $ hg pull
  pulling from $TESTTMP/A3/server (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit B1
  created new head
  $ mkcommit A1
  $ hg debugobsolete `getid "desc(A0)" ` `getid "desc(A1)"`
  $ hg debugobsolete `getid "desc(B0)" ` `getid "desc(B1)"`
  $ hg log -G --hidden
  @  c1c7524e9488 (draft): A1
  |
  o  25c56d33e4c4 (draft): B1
  |
  | x  d73caddc5533 (draft): B0
  | |
  | x  8aaa48160adc (draft): A0
  |/
  o  1e4be0697311 (public): root
  

Actual testing
--------------

  $ hg push
  pushing to $TESTTMP/A3/server (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files (+1 heads)
  2 new obsolescence markers

  $ cd ../..
