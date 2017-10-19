============================================
Testing obsolescence markers push: Cases A.5
============================================

Mercurial pushes obsolescences markers relevant to the "pushed-set", the set of
all changesets that requested to be "in sync" after the push (even if they are
already on both side).

This test belongs to a series of tests checking such set is properly computed
and applied. This does not tests "obsmarkers" discovery capabilities.

Category A: simple cases
TestCase 5: partial reordering

A.5 partial reordering
======================

..
.. {{{
..   B ø⇠⇠
..     | ⇡
..   A ø⇠⇠⇠○ A'
..     | ⇡/
..     | ○ B'
..     |/
..     ● O
.. }}}
..
.. Marker exist from:
..
..  * `Aø⇠○ A'`
..  * `Bø⇠○ B'`
..
.. Command run:
..
..  * hg push -r B
..
.. Expected exchange:
..
..  * `Bø⇠○ B'`
..
.. Expected Exclude:
..
..  * `Aø⇠○ A'`

Setup
-----

  $ . $TESTDIR/testlib/exchange-obsmarker-util.sh

initial

  $ setuprepos A.5
  creating test repo for test case A.5
  - pulldest
  - main
  - pushdest
  cd into `main` and proceed with env setup
  $ cd main
  $ mkcommit A0
  $ mkcommit B0
  $ hg update 0
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ mkcommit B1
  created new head
  $ mkcommit A1
  $ hg debugobsolete aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa `getid 'desc(A0)'`
  $ hg debugobsolete `getid 'desc(B0)'` `getid 'desc(B1)'`
  obsoleted 1 changesets
  $ hg debugobsolete `getid 'desc(A0)'` `getid 'desc(A1)'`
  obsoleted 1 changesets
  $ hg log -G --hidden
  @  8c0a98c83722 (draft): A1
  |
  o  f6298a8ac3a4 (draft): B1
  |
  | x  6e72f0a95b5e (draft): B0
  | |
  | x  28b51eb45704 (draft): A0
  |/
  o  a9bdc8b26820 (public): O
  
  $ inspect_obsmarkers
  obsstore content
  ================
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa 28b51eb45704506b5c603decd6bf7ac5e0f6a52f 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  6e72f0a95b5e01a7504743aa941f69cb1fbef8b0 f6298a8ac3a4b78bbeae5f1d3dc5bc3c3812f0f3 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  28b51eb45704506b5c603decd6bf7ac5e0f6a52f 8c0a98c8372212c6efde4bfdcef006f27ff759d3 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  $ cd ..
  $ cd ..

Actual Test
-----------

  $ dotest A.5 B1
  ## Running testcase A.5
  # testing echange of "B1" (f6298a8ac3a4)
  ## initial state
  # obstore: main
  28b51eb45704506b5c603decd6bf7ac5e0f6a52f 8c0a98c8372212c6efde4bfdcef006f27ff759d3 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  6e72f0a95b5e01a7504743aa941f69cb1fbef8b0 f6298a8ac3a4b78bbeae5f1d3dc5bc3c3812f0f3 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa 28b51eb45704506b5c603decd6bf7ac5e0f6a52f 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pushdest
  # obstore: pulldest
  ## pushing "B1" from main to pushdest
  pushing to pushdest
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  remote: 1 new obsolescence markers
  ## post push state
  # obstore: main
  28b51eb45704506b5c603decd6bf7ac5e0f6a52f 8c0a98c8372212c6efde4bfdcef006f27ff759d3 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  6e72f0a95b5e01a7504743aa941f69cb1fbef8b0 f6298a8ac3a4b78bbeae5f1d3dc5bc3c3812f0f3 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa 28b51eb45704506b5c603decd6bf7ac5e0f6a52f 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pushdest
  6e72f0a95b5e01a7504743aa941f69cb1fbef8b0 f6298a8ac3a4b78bbeae5f1d3dc5bc3c3812f0f3 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pulldest
  ## pulling "f6298a8ac3a4" from main into pulldest
  pulling from main
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  1 new obsolescence markers
  new changesets f6298a8ac3a4
  (run 'hg update' to get a working copy)
  ## post pull state
  # obstore: main
  28b51eb45704506b5c603decd6bf7ac5e0f6a52f 8c0a98c8372212c6efde4bfdcef006f27ff759d3 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  6e72f0a95b5e01a7504743aa941f69cb1fbef8b0 f6298a8ac3a4b78bbeae5f1d3dc5bc3c3812f0f3 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa 28b51eb45704506b5c603decd6bf7ac5e0f6a52f 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pushdest
  6e72f0a95b5e01a7504743aa941f69cb1fbef8b0 f6298a8ac3a4b78bbeae5f1d3dc5bc3c3812f0f3 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pulldest
  6e72f0a95b5e01a7504743aa941f69cb1fbef8b0 f6298a8ac3a4b78bbeae5f1d3dc5bc3c3812f0f3 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
