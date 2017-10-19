============================================
Testing obsolescence markers push: Cases D.1
============================================

Mercurial pushes obsolescences markers relevant to the "pushed-set", the set of
all changesets that requested to be "in sync" after the push (even if they are
already on both side).

This test belongs to a series of tests checking such set is properly computed
and applied. This does not tests "obsmarkers" discovery capabilities.

Category D: Partial Information Case
TestCase 1: Pruned changeset based on missing precursor of something not present
Variants:
# a: explicite push
# b: bare push

D.1 Pruned changeset based on missing precursor of something not present
========================================================================

.. {{{
..   B ⊗
..     |
..   A ◌⇠◔ A'
..     |/
..     ● O
.. }}}
..
.. Markers exist from:
..
..  * `A ø⇠o A'`
..  * B (prune)
..
.. Command run:
..
..  * hg push -r A'
..  * hg push
..
.. Expected exchange:
..
..  * `A ø⇠o A'`
..  * B (prune)

Setup
-----

  $ . $TESTDIR/testlib/exchange-obsmarker-util.sh

initial

  $ setuprepos D.1
  creating test repo for test case D.1
  - pulldest
  - main
  - pushdest
  cd into `main` and proceed with env setup
  $ cd main
  $ mkcommit A0
  $ mkcommit B
  $ hg up -q 0
  $ mkcommit A1
  created new head
  $ hg debugobsolete `getid 'desc(A0)'` `getid 'desc(A1)'`
  obsoleted 1 changesets
  $ hg prune -d '0 0' 'desc(B)'
  obsoleted 1 changesets
  $ hg strip --hidden -q 'desc(A0)'
  $ hg log -G --hidden
  @  e5ea8f9c7314 (draft): A1
  |
  o  a9bdc8b26820 (public): O
  
  $ inspect_obsmarkers
  obsstore content
  ================
  28b51eb45704506b5c603decd6bf7ac5e0f6a52f e5ea8f9c73143125d36658e90ef70c6d2027a5b7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  06055a7959d4128e6e3bccfd01482e83a2db8a3a 0 {28b51eb45704506b5c603decd6bf7ac5e0f6a52f} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  $ cd ..
  $ cd ..

  $ cp -R D.1 D.1.a
  $ cp -R D.1 D.1.b

Actual Test (explicit push)
---------------------------

  $ dotest D.1.a A1
  ## Running testcase D.1.a
  # testing echange of "A1" (e5ea8f9c7314)
  ## initial state
  # obstore: main
  06055a7959d4128e6e3bccfd01482e83a2db8a3a 0 {28b51eb45704506b5c603decd6bf7ac5e0f6a52f} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  28b51eb45704506b5c603decd6bf7ac5e0f6a52f e5ea8f9c73143125d36658e90ef70c6d2027a5b7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pushdest
  # obstore: pulldest
  ## pushing "A1" from main to pushdest
  pushing to pushdest
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  remote: 2 new obsolescence markers
  ## post push state
  # obstore: main
  06055a7959d4128e6e3bccfd01482e83a2db8a3a 0 {28b51eb45704506b5c603decd6bf7ac5e0f6a52f} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  28b51eb45704506b5c603decd6bf7ac5e0f6a52f e5ea8f9c73143125d36658e90ef70c6d2027a5b7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pushdest
  06055a7959d4128e6e3bccfd01482e83a2db8a3a 0 {28b51eb45704506b5c603decd6bf7ac5e0f6a52f} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  28b51eb45704506b5c603decd6bf7ac5e0f6a52f e5ea8f9c73143125d36658e90ef70c6d2027a5b7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pulldest
  ## pulling "e5ea8f9c7314" from main into pulldest
  pulling from main
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  2 new obsolescence markers
  new changesets e5ea8f9c7314
  (run 'hg update' to get a working copy)
  ## post pull state
  # obstore: main
  06055a7959d4128e6e3bccfd01482e83a2db8a3a 0 {28b51eb45704506b5c603decd6bf7ac5e0f6a52f} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  28b51eb45704506b5c603decd6bf7ac5e0f6a52f e5ea8f9c73143125d36658e90ef70c6d2027a5b7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pushdest
  06055a7959d4128e6e3bccfd01482e83a2db8a3a 0 {28b51eb45704506b5c603decd6bf7ac5e0f6a52f} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  28b51eb45704506b5c603decd6bf7ac5e0f6a52f e5ea8f9c73143125d36658e90ef70c6d2027a5b7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pulldest
  06055a7959d4128e6e3bccfd01482e83a2db8a3a 0 {28b51eb45704506b5c603decd6bf7ac5e0f6a52f} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  28b51eb45704506b5c603decd6bf7ac5e0f6a52f e5ea8f9c73143125d36658e90ef70c6d2027a5b7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}

Actual Test (base push)
---------------------------

  $ dotest D.1.b
  ## Running testcase D.1.b
  ## initial state
  # obstore: main
  06055a7959d4128e6e3bccfd01482e83a2db8a3a 0 {28b51eb45704506b5c603decd6bf7ac5e0f6a52f} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  28b51eb45704506b5c603decd6bf7ac5e0f6a52f e5ea8f9c73143125d36658e90ef70c6d2027a5b7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pushdest
  # obstore: pulldest
  ## pushing from main to pushdest
  pushing to pushdest
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  remote: 2 new obsolescence markers
  ## post push state
  # obstore: main
  06055a7959d4128e6e3bccfd01482e83a2db8a3a 0 {28b51eb45704506b5c603decd6bf7ac5e0f6a52f} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  28b51eb45704506b5c603decd6bf7ac5e0f6a52f e5ea8f9c73143125d36658e90ef70c6d2027a5b7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pushdest
  06055a7959d4128e6e3bccfd01482e83a2db8a3a 0 {28b51eb45704506b5c603decd6bf7ac5e0f6a52f} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  28b51eb45704506b5c603decd6bf7ac5e0f6a52f e5ea8f9c73143125d36658e90ef70c6d2027a5b7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pulldest
  ## pulling from main into pulldest
  pulling from main
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  2 new obsolescence markers
  new changesets e5ea8f9c7314
  (run 'hg update' to get a working copy)
  ## post pull state
  # obstore: main
  06055a7959d4128e6e3bccfd01482e83a2db8a3a 0 {28b51eb45704506b5c603decd6bf7ac5e0f6a52f} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  28b51eb45704506b5c603decd6bf7ac5e0f6a52f e5ea8f9c73143125d36658e90ef70c6d2027a5b7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pushdest
  06055a7959d4128e6e3bccfd01482e83a2db8a3a 0 {28b51eb45704506b5c603decd6bf7ac5e0f6a52f} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  28b51eb45704506b5c603decd6bf7ac5e0f6a52f e5ea8f9c73143125d36658e90ef70c6d2027a5b7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pulldest
  06055a7959d4128e6e3bccfd01482e83a2db8a3a 0 {28b51eb45704506b5c603decd6bf7ac5e0f6a52f} (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  28b51eb45704506b5c603decd6bf7ac5e0f6a52f e5ea8f9c73143125d36658e90ef70c6d2027a5b7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}

