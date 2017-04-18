====================================
Testing head checking code: Case D-1
====================================

Mercurial checks for the introduction of new heads on push. Evolution comes
into play to detect if existing branches on the server are being replaced by
some of the new one we push.

This case is part of a series of tests checking this behavior.

Category D: remote head is "obs-affected" locally, but result is not part of the push
TestCase 1: remote head is rewritten, but successors is not part of the push

.. old-state:
..
.. * 1 changeset branch
..
.. new-state:
..
.. * 1 changeset branch succeeding the old branch
.. * 1 new unrelated branch
..
.. expected-result:
..
.. * pushing only the unrelated branch: denied
..
.. graph-summary:
..
..   A ø⇠○ A'
..     |/
..     | ◔ B
..     |/
..     ●

  $ . $TESTDIR/testlib/push-checkheads-util.sh

Test setup
----------

  $ mkdir D1
  $ cd D1
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
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit B0
  created new head
  $ hg log -G --hidden
  @  74ff5441d343 (draft): B0
  |
  | o  f6082bc4ffef (draft): A1
  |/
  | x  8aaa48160adc (draft): A0
  |/
  o  1e4be0697311 (public): root
  

Actual testing
--------------

  $ hg push -r 'desc(B0)'
  pushing to $TESTTMP/D1/server (glob)
  searching for changes
  abort: push creates new remote head 74ff5441d343!
  (merge or see 'hg help push' for details about pushing new heads)
  [255]

  $ cd ../..

