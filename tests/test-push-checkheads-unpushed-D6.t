====================================
Testing head checking code: Case D-6
====================================

Mercurial checks for the introduction of new heads on push. Evolution comes
into play to detect if existing branches on the server are being replaced by
some of the new one we push.

This case is part of a series of tests checking this behavior.

Category D: remote head is "obs-affected" locally, but result is not part of the push
TestCase 6: single changeset, superseeded then pruned (on a new changeset unpushed) changeset

This is a partial push variation of case B-6

.. old-state:
..
.. * 1 changeset branch
..
.. new-state:
..
.. * old branch is rewritten onto another one,
.. * the new version is then pruned.
..
.. expected-result:
..
.. * push denied
..
.. graph-summary:
..
..   A ø⇠⊗ A'
..     | |
.. C ◔ | ○ B
..    \|/
..     ●

  $ . $TESTDIR/testlib/push-checkheads-util.sh

Test setup
----------

  $ mkdir D6
  $ cd D6
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
  $ hg up '0'
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ mkcommit C0
  created new head
  $ hg debugobsolete `getid "desc(A0)"` `getid "desc(A1)"`
  $ hg debugobsolete --record-parents `getid "desc(A1)"`
  $ hg log -G --hidden
  @  0f88766e02d6 (draft): C0
  |
  | x  ba93660aff8d (draft): A1
  | |
  | o  74ff5441d343 (draft): B0
  |/
  | x  8aaa48160adc (draft): A0
  |/
  o  1e4be0697311 (public): root
  

Actual testing
--------------

  $ hg push --rev 'desc(C0)'
  pushing to $TESTTMP/D6/server (glob)
  searching for changes
  abort: push creates new remote head 0f88766e02d6!
  (merge or see 'hg help push' for details about pushing new heads)
  [255]

  $ cd ../..
