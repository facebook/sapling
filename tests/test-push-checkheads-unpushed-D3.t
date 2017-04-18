====================================
Testing head checking code: Case D-3
====================================

Mercurial checks for the introduction of new heads on push. Evolution comes
into play to detect if existing branches on the server are being replaced by
some of the new one we push.

This case is part of a series of tests checking this behavior.

Category D: remote head is "obs-affected" locally, but result is not part of the push
TestCase 3: multi-changeset branch, split on multiple new others, only one of them is pushed

.. old-state:
..
.. * 2 changesets branch
..
.. new-state:
..
.. * 2 new branches, each superseding one changeset in the old one.
..
.. expected-result:
..
.. * pushing only one of the resulting branch (either of them)
.. * push denied
..
.. graph-summary:
..
.. B'◔⇢ø B
..   | |
.. A | ø⇠◔ A'
..   | |/
..    \|
..     ●

  $ . $TESTDIR/testlib/push-checkheads-util.sh

Test setup
----------

  $ mkdir D3
  $ cd D3
  $ setuprepos
  creating basic server and client repo
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd server
  $ mkcommit B0
  $ hg up 0
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ cd ../client
  $ hg pull
  pulling from $TESTTMP/D3/server (glob)
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
  $ hg up '0'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit B1
  created new head
  $ hg debugobsolete `getid "desc(A0)" ` `getid "desc(A1)"`
  $ hg debugobsolete `getid "desc(B0)" ` `getid "desc(B1)"`
  $ hg log -G --hidden
  @  25c56d33e4c4 (draft): B1
  |
  | o  f6082bc4ffef (draft): A1
  |/
  | x  d73caddc5533 (draft): B0
  | |
  | x  8aaa48160adc (draft): A0
  |/
  o  1e4be0697311 (public): root
  

Actual testing
--------------

  $ hg push --rev 'desc(A1)'
  pushing to $TESTTMP/D3/server (glob)
  searching for changes
  abort: push creates new remote head f6082bc4ffef!
  (merge or see 'hg help push' for details about pushing new heads)
  [255]
  $ hg push --rev 'desc(B1)'
  pushing to $TESTTMP/D3/server (glob)
  searching for changes
  abort: push creates new remote head 25c56d33e4c4!
  (merge or see 'hg help push' for details about pushing new heads)
  [255]

Extra testing
-------------

In this case, even a bare push is creating more heads

  $ hg push
  pushing to $TESTTMP/D3/server (glob)
  searching for changes
  abort: push creates new remote head 25c56d33e4c4!
  (merge or see 'hg help push' for details about pushing new heads)
  [255]

  $ cd ../..
