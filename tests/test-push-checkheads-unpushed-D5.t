====================================
Testing head checking code: Case D-5
====================================

Mercurial checks for the introduction of new heads on push. Evolution comes
into play to detect if existing branches on the server are being replaced by
some of the new one we push.

This case is part of a series of tests checking this behavior.

Category D: remote head is "obs-affected" locally, but result is not part of the push
TestCase 5: multi-changeset branch, split on multiple other, (head on its own new branch)

.. old-state:
..
.. * 2 branch (1 changeset, and 2 changesets)
..
.. new-state:
..
.. * 1 new branch superceeding the head of the old-2-changesets-branch,
.. * 1 new changesets on the old-1-changeset-branch superceeding the base of the other
..
.. expected-result:
..
.. * push the new branch only -> push denied
.. * push the existing branch only -> push allowed
..   /!\ This push create unstability/orphaning on the other hand and we should
..  probably detect/warn agains that.
..
.. graph-summary:
..
..   B ø⇠◔ B'
..     | |
.. A'◔⇢ø |
..   | |/
.. C ● |
..    \|
..     ●

  $ . $TESTDIR/testlib/push-checkheads-util.sh

Test setup
----------

  $ mkdir D5
  $ cd D5
  $ setuprepos
  creating basic server and client repo
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd server
  $ mkcommit B0
  $ hg up 0
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ mkcommit C0
  created new head
  $ cd ../client
  $ hg pull
  pulling from $TESTTMP/D5/server (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg up 'desc(C0)'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit A1
  $ hg up 0
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ mkcommit B1
  created new head
  $ hg debugobsolete `getid "desc(A0)" ` `getid "desc(A1)"`
  $ hg debugobsolete `getid "desc(B0)" ` `getid "desc(B1)"`
  $ hg log -G --hidden
  @  25c56d33e4c4 (draft): B1
  |
  | o  a0802eb7fc1b (draft): A1
  | |
  | o  0f88766e02d6 (draft): C0
  |/
  | x  d73caddc5533 (draft): B0
  | |
  | x  8aaa48160adc (draft): A0
  |/
  o  1e4be0697311 (public): root
  

Actual testing
--------------

  $ hg push --rev 'desc(B1)'
  pushing to $TESTTMP/D5/server (glob)
  searching for changes
  abort: push creates new remote head 25c56d33e4c4!
  (merge or see 'hg help push' for details about pushing new heads)
  [255]
  $ hg push --rev 'desc(A1)'
  pushing to $TESTTMP/D5/server (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  1 new obsolescence markers

  $ cd ../..
