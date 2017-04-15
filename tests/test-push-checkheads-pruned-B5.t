====================================
Testing head checking code: Case B-5
====================================

Mercurial checks for the introduction of new heads on push. Evolution comes
into play to detect if existing branches on the server are being replaced by
some of the new one we push.

This case is part of a series of tests checking this behavior.

Category B: simple case involving pruned changesets
TestCase 5: multi-changeset branch, mix of pruned and superceeded

.. old-state:
..
.. * 3 changeset branch
..
.. new-state:
..
.. * old head is pruned
.. * old mid is superceeded
.. * old root is pruned
..
.. expected-result:
..
.. * push allowed
..
.. graph-summary:
..
..   B ⊗
..     |
..   A ø⇠◔ A'
..     | |
..   B ⊗ |
..     |/
..     ●

  $ . $TESTDIR/testlib/push-checkheads-util.sh

Test setup
----------

  $ mkdir B5
  $ cd B5
  $ setuprepos
  creating basic server and client repo
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd server
  $ mkcommit B0
  $ mkcommit C0
  $ cd ../client
  $ hg pull
  pulling from $TESTTMP/B5/server (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  (run 'hg update' to get a working copy)
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit B1
  created new head
  $ hg debugobsolete --record-parents `getid "desc(A0)"`
  $ hg debugobsolete `getid "desc(B0)"` `getid "desc(B1)"`
  $ hg debugobsolete --record-parents `getid "desc(C0)"`
  $ hg log -G --hidden
  @  25c56d33e4c4 (draft): B1
  |
  | x  821fb21d0dd2 (draft): C0
  | |
  | x  d73caddc5533 (draft): B0
  | |
  | x  8aaa48160adc (draft): A0
  |/
  o  1e4be0697311 (public): root
  

Actual testing
--------------

  $ hg push
  pushing to $TESTTMP/B5/server (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  3 new obsolescence markers

  $ cd ../..
