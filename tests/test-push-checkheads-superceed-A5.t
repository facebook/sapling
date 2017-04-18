====================================
Testing head checking code: Case A-5
====================================

Mercurial checks for the introduction of new heads on push. Evolution comes
into play to detect if existing branches on the server are being replaced by
some of the new one we push.

This case is part of a series of tests checking this behavior.

Category A: simple case involving a branch being superceeded by another.
TestCase 5: New changeset as parent of the successor

.. old-state:
..
.. * 1-changeset branch
..
.. new-state:
..
.. * 2rchangeset branch, head is a successor, but other is new
..
.. expected-result:
..
.. * push allowed
..
.. graph-summary:
..
..   A ø⇠◔ A'
..     | |
..     | ◔ B
..     |/
..     ●

  $ . $TESTDIR/testlib/push-checkheads-util.sh

Test setup
----------

  $ mkdir A5
  $ cd A5
  $ setuprepos
  creating basic server and client repo
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd client
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit B0
  created new head
  $ mkcommit A1
  $ hg debugobsolete `getid "desc(A0)" ` `getid "desc(A1)"`
  $ hg log -G --hidden
  @  ba93660aff8d (draft): A1
  |
  o  74ff5441d343 (draft): B0
  |
  | x  8aaa48160adc (draft): A0
  |/
  o  1e4be0697311 (public): root
  

Actual testing
--------------

  $ hg push
  pushing to $TESTTMP/A5/server (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files (+1 heads)
  1 new obsolescence markers

  $ cd ../..

