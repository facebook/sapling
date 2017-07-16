============================================
Testing obsolescence markers push: Cases D.3
============================================

Mercurial pushes obsolescences markers relevant to the "pushed-set", the set of
all changesets that requested to be "in sync" after the push (even if they are
already on both side).

This test belongs to a series of tests checking such set is properly computed
and applied. This does not tests "obsmarkers" discovery capabilities.

Category D: Partial Information Case
TestCase 3: missing prune target (prune not in "pushed set")

D.3 missing prune target (prune not in "pushed set")
====================================================

.. {{{
..  A ø⇠✕ A'
..     | |
..     | ○ B
..     |/
..     ● O
.. }}}
..
.. Marker exist from:
..
..  * `A ø⇠o A'`
..  * A' (prune)
..
.. Command runs:
..
..  * hg push
..
.. Expected exclude:
..
..  * `A ø⇠o A'`
..  * A' (prune)

Setup
-----

  $ . $TESTDIR/testlib/exchange-obsmarker-util.sh

initial

  $ setuprepos D.3
  creating test repo for test case D.3
  - pulldest
  - main
  - pushdest
  cd into `main` and proceed with env setup
  $ cd main
  $ mkcommit A0
  $ hg up -q 0
  $ mkcommit B
  created new head
  $ mkcommit A1
  $ hg debugobsolete `getid 'desc(A0)'` `getid 'desc(A1)'`
  obsoleted 1 changesets
  $ hg prune -d '0 0' .
  obsoleted 1 changesets
  $ hg strip --hidden -q 'desc(A1)'
  $ hg log -G --hidden
  @  35b183996678 (draft): B
  |
  | x  28b51eb45704 (draft): A0
  |/
  o  a9bdc8b26820 (public): O
  
  $ inspect_obsmarkers
  obsstore content
  ================
  28b51eb45704506b5c603decd6bf7ac5e0f6a52f 6aa67a7b4baa6fb41b06aed38d5b1201436546e2 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  6aa67a7b4baa6fb41b06aed38d5b1201436546e2 0 {35b1839966785d5703a01607229eea932db42f87} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  $ cd ..
  $ cd ..

Actual Test
-----------

  $ dotest D.3 O
  ## Running testcase D.3
  # testing echange of "O" (a9bdc8b26820)
  ## initial state
  # obstore: main
  28b51eb45704506b5c603decd6bf7ac5e0f6a52f 6aa67a7b4baa6fb41b06aed38d5b1201436546e2 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  6aa67a7b4baa6fb41b06aed38d5b1201436546e2 0 {35b1839966785d5703a01607229eea932db42f87} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pushdest
  # obstore: pulldest
  ## pushing "O" from main to pushdest
  pushing to pushdest
  searching for changes
  no changes found
  ## post push state
  # obstore: main
  28b51eb45704506b5c603decd6bf7ac5e0f6a52f 6aa67a7b4baa6fb41b06aed38d5b1201436546e2 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  6aa67a7b4baa6fb41b06aed38d5b1201436546e2 0 {35b1839966785d5703a01607229eea932db42f87} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pushdest
  # obstore: pulldest
  ## pulling "a9bdc8b26820" from main into pulldest
  pulling from main
  no changes found
  ## post pull state
  # obstore: main
  28b51eb45704506b5c603decd6bf7ac5e0f6a52f 6aa67a7b4baa6fb41b06aed38d5b1201436546e2 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  6aa67a7b4baa6fb41b06aed38d5b1201436546e2 0 {35b1839966785d5703a01607229eea932db42f87} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pushdest
  # obstore: pulldest

