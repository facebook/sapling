====================================
Testing head checking code: Case B-4
====================================

Mercurial checks for the introduction of new heads on push. Evolution comes
into play to detect if existing branches on the server are being replaced by
some of the new one we push.

This case is part of a series of tests checking this behavior.

Category B: simple case involving pruned changesets
TestCase 4: multi-changeset branch, all are pruned

.. old-state:
..
.. * 2 changeset branch
..
.. new-state:
..
.. * old branch is pruned
..
.. expected-result:
..
.. * push allowed
..
.. graph-summary:
..
..   B ⊗
..     |
..   A ⊗
..     |
..     | ◔ C
..     |/
..     ●

  $ . $TESTDIR/testlib/push-checkheads-util.sh

Test setup
----------

  $ mkdir B4
  $ cd B4
  $ setuprepos
  creating basic server and client repo
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd server
  $ mkcommit B0
  $ cd ../client
  $ hg pull
  pulling from $TESTTMP/B4/server (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit C0
  created new head
  $ hg debugobsolete --record-parents `getid "desc(A0)"`
  $ hg debugobsolete --record-parents `getid "desc(B0)"`
  $ hg log -G --hidden
  @  0f88766e02d6 (draft): C0
  |
  | x  d73caddc5533 (draft): B0
  | |
  | x  8aaa48160adc (draft): A0
  |/
  o  1e4be0697311 (public): root
  

Actual testing
--------------

  $ hg push
  pushing to $TESTTMP/B4/server (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  2 new obsolescence markers

  $ cd ../..
