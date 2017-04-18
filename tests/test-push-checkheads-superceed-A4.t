====================================
Testing head checking code: Case A-4
====================================

Mercurial checks for the introduction of new heads on push. Evolution comes
into play to detect if existing branches on the server are being replaced by
some of the new one we push.

This case is part of a series of tests checking this behavior.

Category A: simple case involving a branch being superceeded by another.
TestCase 4: New changeset as children of the successor

.. old-state:
..
.. * 1-changeset branch
..
.. new-state:
..
.. * 2-changeset branch, first is a successor, but head is new
..
.. expected-result:
..
.. * push allowed
..
.. graph-summary:
..
..       ◔ B
..       |
..   A ø⇠◔ A'
..     |/
..     ●

  $ . $TESTDIR/testlib/push-checkheads-util.sh

Test setup
----------

  $ mkdir A4
  $ cd A4
  $ setuprepos
  creating basic server and client repo
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd client
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit A1
  created new head
  $ hg debugobsolete `getid "desc(A0)" ` `getid "desc(A1)"`
  $ mkcommit B0
  $ hg log -G --hidden
  @  f40ded968333 (draft): B0
  |
  o  f6082bc4ffef (draft): A1
  |
  | x  8aaa48160adc (draft): A0
  |/
  o  1e4be0697311 (public): root
  

Actual testing
--------------

  $ hg push
  pushing to $TESTTMP/A4/server (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files (+1 heads)
  1 new obsolescence markers

  $ cd ../../
