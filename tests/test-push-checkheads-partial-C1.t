====================================
Testing head checking code: Case C-2
====================================

Mercurial checks for the introduction of new heads on push. Evolution comes
into play to detect if existing branches on the server are being replaced by
some of the new one we push.

This case is part of a series of tests checking this behavior.

Category C: checking case were the branch is only partially obsoleted.
TestCase 1: 2 changeset branch, only the head is rewritten

.. old-state:
..
.. * 2 changeset branch
..
.. new-state:
..
.. * 1 new changesets branches superceeding only the head of the old one
.. * base of the old branch is still alive
..
.. expected-result:
..
.. * push denied
..
.. graph-summary:
..
..   B ø⇠◔ B'
..     | |
..   A ○ |
..     |/
..     ○

  $ . $TESTDIR/testlib/push-checkheads-util.sh

Test setup
----------

  $ mkdir C1
  $ cd C1
  $ setuprepos
  creating basic server and client repo
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd server
  $ mkcommit B0
  $ cd ../client
  $ hg pull
  pulling from $TESTTMP/C1/server (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets d73caddc5533
  (run 'hg update' to get a working copy)
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit B1
  created new head
  $ hg debugobsolete `getid "desc(B0)" ` `getid "desc(B1)"`
  obsoleted 1 changesets
  $ hg log -G --hidden
  @  25c56d33e4c4 (draft): B1
  |
  | x  d73caddc5533 (draft): B0
  | |
  | o  8aaa48160adc (draft): A0
  |/
  o  1e4be0697311 (public): root
  

Actual testing
--------------

  $ hg push
  pushing to $TESTTMP/C1/server (glob)
  searching for changes
  abort: push creates new remote head 25c56d33e4c4!
  (merge or see 'hg help push' for details about pushing new heads)
  [255]

  $ cd ../..
