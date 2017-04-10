============================================
Testing obsolescence markers push: Cases A.7
============================================

Mercurial pushes obsolescences markers relevant to the "pushed-set", the set of
all changesets that requested to be "in sync" after the push (even if they are
already on both side).

This test belongs to a series of tests checking such set is properly computed
and applied. This does not tests "obsmarkers" discovery capabilities.

Category A: simple cases
TestCase 7: markers one non targeted common changeset

A.7 non targeted common changeset
=================================

.. {{{
..    ⇠◕ A
..     |
..     ● O
.. }}}
..
.. Markers exist from:
..
..  * Chain from A
..
.. Command run:
..
..  * hg push -r O
..
.. Expected exchange:
..
..  * ø

Setup
-----

  $ . $TESTDIR/testlib/exchange-obsmarker-util.sh

Initial

  $ setuprepos A.7
  creating test repo for test case A.7
  - pulldest
  - main
  - pushdest
  cd into `main` and proceed with env setup
  $ cd main
  $ mkcommit A
  $ hg push -q ../pushdest
  $ hg push -q ../pulldest
  $ hg debugobsolete aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa `getid 'desc(A)'`
  $ hg log -G --hidden
  @  f5bc6836db60 (draft): A
  |
  o  a9bdc8b26820 (public): O
  
  $ inspect_obsmarkers
  obsstore content
  ================
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa f5bc6836db60e308a17ba08bf050154ba9c4fad7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  $ cd ..
  $ cd ..

Actual Test
-----------------------------------

  $ dotest A.7 O
  ## Running testcase A.7
  # testing echange of "O" (a9bdc8b26820)
  ## initial state
  # obstore: main
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa f5bc6836db60e308a17ba08bf050154ba9c4fad7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pushdest
  # obstore: pulldest
  ## pushing "O" from main to pushdest
  pushing to pushdest
  searching for changes
  no changes found
  ## post push state
  # obstore: main
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa f5bc6836db60e308a17ba08bf050154ba9c4fad7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pushdest
  # obstore: pulldest
  ## pulling "a9bdc8b26820" from main into pulldest
  pulling from main
  no changes found
  ## post pull state
  # obstore: main
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa f5bc6836db60e308a17ba08bf050154ba9c4fad7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pushdest
  # obstore: pulldest
