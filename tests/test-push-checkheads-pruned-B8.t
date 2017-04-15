====================================
Testing head checking code: Case B-2
====================================

Mercurial checks for the introduction of new heads on push. Evolution comes
into play to detect if existing branches on the server are being replaced by
some of the new one we push.

This case is part of a series of tests checking this behavior.

Category B: simple case involving pruned changesets
TestCase 2: multi-changeset branch, head is pruned, rest is superceeded, through other

.. old-state:
..
.. * 2 changeset branch
..
.. new-state:
..
.. * old head is rewritten then pruned
.. * 1 new branch succeeding to the other changeset in the old branch (through another obsolete branch)
..
.. expected-result:
..
.. * push allowed
..
.. graph-summary:
..
..   B ø⇠⊗ B'
..     | | A'
..   A ø⇠ø⇠◔ A''
..     |/ /
..     | /
..     |/
..     ●

  $ . $TESTDIR/testlib/push-checkheads-util.sh

Test setup
----------

  $ mkdir B8
  $ cd B8
  $ setuprepos
  creating basic server and client repo
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd server
  $ mkcommit B0
  $ cd ../client
  $ hg pull
  pulling from $TESTTMP/B8/server (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit A1
  created new head
  $ mkcommit B1
  $ hg up 0
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ mkcommit A2
  created new head
  $ hg debugobsolete `getid "desc(A0)" ` `getid "desc(A1)"`
  $ hg debugobsolete `getid "desc(B0)" ` `getid "desc(B1)"`
  $ hg debugobsolete --record-parents `getid "desc(B1)"`
  $ hg debugobsolete `getid "desc(A1)" ` `getid "desc(A2)"`
  $ hg log -G --hidden
  @  c1f8d089020f (draft): A2
  |
  | x  262c8c798096 (draft): B1
  | |
  | x  f6082bc4ffef (draft): A1
  |/
  | x  d73caddc5533 (draft): B0
  | |
  | x  8aaa48160adc (draft): A0
  |/
  o  1e4be0697311 (public): root
  

Actual testing
--------------

  $ hg push
  pushing to $TESTTMP/B8/server (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  4 new obsolescence markers

  $ cd ../..
