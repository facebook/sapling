============================================
Testing obsolescence markers push: Cases A.2
============================================

Mercurial pushes obsolescences markers relevant to the "pushed-set", the set of
all changesets that requested to be "in sync" after the push (even if they are
already on both side).

This test belongs to a series of tests checking such set is properly computed
and applied. This does not tests "obsmarkers" discovery capabilities.

Category A: simple cases
TestCase 2: Two heads, only one of them pushed

A.2 Two heads, only on of then pushed
=====================================

.. {{{
..     ⇠○ B
..   ⇠◔ | A
..    |/
..    ● O
.. }}}
..
.. Markers exist from:
..
..  * A
..  * B
..
..
.. Command runs:
..
..  * hg push -r A
..
.. Expected exchange:
..
..  * chain from A
..
.. Expected Exclude:
..
..  * chain from B

Setup
-----

  $ . $TESTDIR/testlib/exchange-obsmarker-util.sh

initial

  $ setuprepos A.2
  creating test repo for test case A.2
  - pulldest
  - main
  - pushdest
  cd into `main` and proceed with env setup
  $ cd main
  $ mkcommit A
  $ hg debugobsolete aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa `getid 'desc(A)'`
  $ hg up '.~1'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit B
  created new head
  $ hg debugobsolete bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb `getid 'desc(B)'`
  $ hg log -G
  @  35b183996678 (draft): B
  |
  | o  f5bc6836db60 (draft): A
  |/
  o  a9bdc8b26820 (public): O
  
  $ inspect_obsmarkers
  obsstore content
  ================
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa f5bc6836db60e308a17ba08bf050154ba9c4fad7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb 35b1839966785d5703a01607229eea932db42f87 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  $ cd ..
  $ cd ..

Actual Test
-----------

  $ dotest A.2 A
  ## Running testcase A.2
  # testing echange of "A" (f5bc6836db60)
  ## initial state
  # obstore: main
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa f5bc6836db60e308a17ba08bf050154ba9c4fad7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb 35b1839966785d5703a01607229eea932db42f87 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pushdest
  # obstore: pulldest
  ## pushing "A" from main to pushdest
  pushing to pushdest
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  remote: 1 new obsolescence markers
  ## post push state
  # obstore: main
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa f5bc6836db60e308a17ba08bf050154ba9c4fad7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb 35b1839966785d5703a01607229eea932db42f87 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pushdest
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa f5bc6836db60e308a17ba08bf050154ba9c4fad7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pulldest
  ## pulling "f5bc6836db60" from main into pulldest
  pulling from main
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  1 new obsolescence markers
  new changesets f5bc6836db60
  (run 'hg update' to get a working copy)
  ## post pull state
  # obstore: main
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa f5bc6836db60e308a17ba08bf050154ba9c4fad7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb 35b1839966785d5703a01607229eea932db42f87 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pushdest
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa f5bc6836db60e308a17ba08bf050154ba9c4fad7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pulldest
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa f5bc6836db60e308a17ba08bf050154ba9c4fad7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}

  $ cd ..
