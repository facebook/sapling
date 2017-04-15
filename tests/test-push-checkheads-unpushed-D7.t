====================================
Testing head checking code: Case D-7
====================================

Mercurial checks for the introduction of new heads on push. Evolution comes
into play to detect if existing branches on the server are being replaced by
some of the new one we push.

This case is part of a series of tests checking this behavior.

Category D: remote head is "obs-affected" locally, but result is not part of the push
TestCase 7: single changesets, superseeded multiple time then pruned (on a new changeset unpushed) changeset

This is a partial push variation of B6

.. old-state:
..
.. * 1 changeset branch
..
.. new-state:
..
.. * old branch is rewritten onto another one,
.. * The rewriting it again rewritten on the root
.. * the new version is then pruned.
..
.. expected-result:
..
.. * push allowed
..
.. graph-summary:
..
..       A'
..   A ø⇠ø⇠⊗ A''
..     | | |
.. C ◔ | ◔ | B
..    \|/ /
..     | /
..     |/
..     |
..     ●

  $ . $TESTDIR/testlib/push-checkheads-util.sh

Test setup
----------

  $ mkdir D7
  $ cd D7
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
  $ mkcommit A2
  created new head
  $ hg up '0'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit C0
  created new head
  $ hg debugobsolete `getid "desc(A0)"` `getid "desc(A1)"`
  $ hg debugobsolete `getid "desc(A1)"` `getid "desc(A2)"`
  $ hg debugobsolete --record-parents `getid "desc(A2)"`
  $ hg log -G --hidden
  @  0f88766e02d6 (draft): C0
  |
  | x  c1f8d089020f (draft): A2
  |/
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
  pushing to $TESTTMP/D7/server (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  3 new obsolescence markers

  $ cd ../..
