============================================
Testing obsolescence markers push: Cases B.6
============================================

Mercurial pushes obsolescences markers relevant to the "pushed-set", the set of
all changesets that requested to be "in sync" after the push (even if they are
already on both side).

This test belongs to a series of tests checking such set is properly computed
and applied. This does not tests "obsmarkers" discovery capabilities.

Category B: pruning case
TestCase 6: Pruned changeset with precursors not in pushed set

B.6 Pruned changeset with precursors not in pushed set
======================================================

.. {{{
..   B ø⇠⊗ B'
..     | |
..   A ○ |
..     |/
..     ● O
.. }}}
..
.. Marker exist from:
..
..  * `B ø⇠⊗ B'`
..  * B' prune
..
.. Command run:
..
..  * hg push -r O
..
.. Expected exchange:
..
..  * `B ø⇠⊗ B'`
..  * B' prune

Setup
-----

  $ . $TESTDIR/testlib/exchange-obsmarker-util.sh

Initial

  $ setuprepos B.6
  creating test repo for test case B.6
  - pulldest
  - main
  - pushdest
  cd into `main` and proceed with env setup
  $ cd main
  $ mkcommit A
  $ mkcommit B0
  $ hg up --quiet 0
  $ mkcommit B1
  created new head
  $ hg debugobsolete `getid 'desc(B0)'` `getid 'desc(B1)'`
  obsoleted 1 changesets
  $ hg prune -qd '0 0' .
  $ hg log -G --hidden
  x  f6298a8ac3a4 (draft): B1
  |
  | x  962ecf6b1afc (draft): B0
  | |
  | o  f5bc6836db60 (draft): A
  |/
  @  a9bdc8b26820 (public): O
  
  $ inspect_obsmarkers
  obsstore content
  ================
  962ecf6b1afc94e15c7e48fdfb76ef8abd11372b f6298a8ac3a4b78bbeae5f1d3dc5bc3c3812f0f3 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  f6298a8ac3a4b78bbeae5f1d3dc5bc3c3812f0f3 0 {a9bdc8b26820b1b87d585b82eb0ceb4a2ecdbc04} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  $ cd ..
  $ cd ..

Actual Test
-------------------------------------

  $ dotest B.6 O
  ## Running testcase B.6
  # testing echange of "O" (a9bdc8b26820)
  ## initial state
  # obstore: main
  962ecf6b1afc94e15c7e48fdfb76ef8abd11372b f6298a8ac3a4b78bbeae5f1d3dc5bc3c3812f0f3 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  f6298a8ac3a4b78bbeae5f1d3dc5bc3c3812f0f3 0 {a9bdc8b26820b1b87d585b82eb0ceb4a2ecdbc04} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pushdest
  # obstore: pulldest
  ## pushing "O" from main to pushdest
  pushing to pushdest
  searching for changes
  no changes found
  remote: 2 new obsolescence markers
  ## post push state
  # obstore: main
  962ecf6b1afc94e15c7e48fdfb76ef8abd11372b f6298a8ac3a4b78bbeae5f1d3dc5bc3c3812f0f3 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  f6298a8ac3a4b78bbeae5f1d3dc5bc3c3812f0f3 0 {a9bdc8b26820b1b87d585b82eb0ceb4a2ecdbc04} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pushdest
  962ecf6b1afc94e15c7e48fdfb76ef8abd11372b f6298a8ac3a4b78bbeae5f1d3dc5bc3c3812f0f3 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  f6298a8ac3a4b78bbeae5f1d3dc5bc3c3812f0f3 0 {a9bdc8b26820b1b87d585b82eb0ceb4a2ecdbc04} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pulldest
  ## pulling "a9bdc8b26820" from main into pulldest
  pulling from main
  no changes found
  2 new obsolescence markers
  ## post pull state
  # obstore: main
  962ecf6b1afc94e15c7e48fdfb76ef8abd11372b f6298a8ac3a4b78bbeae5f1d3dc5bc3c3812f0f3 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  f6298a8ac3a4b78bbeae5f1d3dc5bc3c3812f0f3 0 {a9bdc8b26820b1b87d585b82eb0ceb4a2ecdbc04} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pushdest
  962ecf6b1afc94e15c7e48fdfb76ef8abd11372b f6298a8ac3a4b78bbeae5f1d3dc5bc3c3812f0f3 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  f6298a8ac3a4b78bbeae5f1d3dc5bc3c3812f0f3 0 {a9bdc8b26820b1b87d585b82eb0ceb4a2ecdbc04} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pulldest
  962ecf6b1afc94e15c7e48fdfb76ef8abd11372b f6298a8ac3a4b78bbeae5f1d3dc5bc3c3812f0f3 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  f6298a8ac3a4b78bbeae5f1d3dc5bc3c3812f0f3 0 {a9bdc8b26820b1b87d585b82eb0ceb4a2ecdbc04} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}

