============================================
Testing obsolescence markers push: Cases A.3
============================================

Mercurial pushes obsolescences markers relevant to the "pushed-set", the set of
all changesets that requested to be "in sync" after the push (even if they are
already on both side).

This test belongs to a series of tests checking such set is properly computed
and applied. This does not tests "obsmarkers" discovery capabilities.

Category A: simple cases
TestCase 3: old branch split in two, only one of the new one pushed
Variants:
# a: changesets are known on remote
# b: changesets are known on remote (push needs -f)

A.3 new branchs created, one pushed.
====================================

.. {{{
..   B' ○⇢ø B
..      | |
..      \Aø⇠◔ A'
..       \|/
..        ● O
.. }}}
..
.. Markers  exist from:
..
..  * `A ø⇠○ A'`
..  * `B ø⇠○ B'`
..
.. Command runs:
..
..  * hg push -r A
..
.. Expected exchange:
..
..  * chain from A
..
.. Expected exclude:
..
..  * chain from B
..
.. Extra note:
..
.. If A and B are remontly known, we should expect:
..
..  * `hg push` will complain about the new head
..  * `hg push` should complain about unstable history creation

Setup
-----

  $ . $TESTDIR/testlib/exchange-obsmarker-util.sh

initial

  $ setuprepos A.3.a
  creating test repo for test case A.3.a
  - pulldest
  - main
  - pushdest
  cd into `main` and proceed with env setup
  $ cd main
  $ mkcommit A0
  $ mkcommit B0
  $ hg update -q 0
  $ mkcommit A1
  created new head
  $ hg update -q 0
  $ mkcommit B1
  created new head
  $ hg debugobsolete `getid 'desc(A0)'` `getid 'desc(A1)'`
  obsoleted 1 changesets
  $ hg debugobsolete `getid 'desc(B0)'` `getid 'desc(B1)'`
  obsoleted 1 changesets
  $ hg log -G --hidden
  @  f6298a8ac3a4 (draft): B1
  |
  | o  e5ea8f9c7314 (draft): A1
  |/
  | x  6e72f0a95b5e (draft): B0
  | |
  | x  28b51eb45704 (draft): A0
  |/
  o  a9bdc8b26820 (public): O
  
  $ inspect_obsmarkers
  obsstore content
  ================
  28b51eb45704506b5c603decd6bf7ac5e0f6a52f e5ea8f9c73143125d36658e90ef70c6d2027a5b7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  6e72f0a95b5e01a7504743aa941f69cb1fbef8b0 f6298a8ac3a4b78bbeae5f1d3dc5bc3c3812f0f3 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  $ cd ..
  $ cd ..

Actual Test for first version (changeset unknown in remote)
-----------------------------------------------------------

  $ dotest A.3.a A1
  ## Running testcase A.3.a
  # testing echange of "A1" (e5ea8f9c7314)
  ## initial state
  # obstore: main
  28b51eb45704506b5c603decd6bf7ac5e0f6a52f e5ea8f9c73143125d36658e90ef70c6d2027a5b7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  6e72f0a95b5e01a7504743aa941f69cb1fbef8b0 f6298a8ac3a4b78bbeae5f1d3dc5bc3c3812f0f3 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pushdest
  # obstore: pulldest
  ## pushing "A1" from main to pushdest
  pushing to pushdest
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  remote: 1 new obsolescence markers
  ## post push state
  # obstore: main
  28b51eb45704506b5c603decd6bf7ac5e0f6a52f e5ea8f9c73143125d36658e90ef70c6d2027a5b7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  6e72f0a95b5e01a7504743aa941f69cb1fbef8b0 f6298a8ac3a4b78bbeae5f1d3dc5bc3c3812f0f3 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pushdest
  28b51eb45704506b5c603decd6bf7ac5e0f6a52f e5ea8f9c73143125d36658e90ef70c6d2027a5b7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pulldest
  ## pulling "e5ea8f9c7314" from main into pulldest
  pulling from main
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  1 new obsolescence markers
  new changesets e5ea8f9c7314
  (run 'hg update' to get a working copy)
  ## post pull state
  # obstore: main
  28b51eb45704506b5c603decd6bf7ac5e0f6a52f e5ea8f9c73143125d36658e90ef70c6d2027a5b7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  6e72f0a95b5e01a7504743aa941f69cb1fbef8b0 f6298a8ac3a4b78bbeae5f1d3dc5bc3c3812f0f3 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pushdest
  28b51eb45704506b5c603decd6bf7ac5e0f6a52f e5ea8f9c73143125d36658e90ef70c6d2027a5b7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pulldest
  28b51eb45704506b5c603decd6bf7ac5e0f6a52f e5ea8f9c73143125d36658e90ef70c6d2027a5b7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}

other variant: changeset known in remote
----------------------------------------

  $ setuprepos A.3.b
  creating test repo for test case A.3.b
  - pulldest
  - main
  - pushdest
  cd into `main` and proceed with env setup
  $ cd main
  $ mkcommit A0
  $ mkcommit B0
  $ hg push -q ../pushdest
  $ hg push -q ../pulldest
  $ hg update -q 0
  $ mkcommit A1
  created new head
  $ hg update -q 0
  $ mkcommit B1
  created new head
  $ hg debugobsolete `getid 'desc(A0)'` `getid 'desc(A1)'`
  obsoleted 1 changesets
  $ hg debugobsolete `getid 'desc(B0)'` `getid 'desc(B1)'`
  obsoleted 1 changesets
  $ hg log -G --hidden
  @  f6298a8ac3a4 (draft): B1
  |
  | o  e5ea8f9c7314 (draft): A1
  |/
  | x  6e72f0a95b5e (draft): B0
  | |
  | x  28b51eb45704 (draft): A0
  |/
  o  a9bdc8b26820 (public): O
  
  $ inspect_obsmarkers
  obsstore content
  ================
  28b51eb45704506b5c603decd6bf7ac5e0f6a52f e5ea8f9c73143125d36658e90ef70c6d2027a5b7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  6e72f0a95b5e01a7504743aa941f69cb1fbef8b0 f6298a8ac3a4b78bbeae5f1d3dc5bc3c3812f0f3 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  $ cd ..
  $ cd ..

Actual Test for first version (changeset known in remote)
-----------------------------------------------------------

check it complains about multiple heads

  $ cd A.3.b
  $ hg push -R main -r 'desc(A1)' pushdest
  pushing to pushdest
  searching for changes
  abort: push creates new remote head e5ea8f9c7314!
  (merge or see 'hg help push' for details about pushing new heads)
  [255]
  $ cd ..

test obsmarkers exchange.

  $ dotest A.3.b A1 -f
  ## Running testcase A.3.b
  # testing echange of "A1" (e5ea8f9c7314)
  ## initial state
  # obstore: main
  28b51eb45704506b5c603decd6bf7ac5e0f6a52f e5ea8f9c73143125d36658e90ef70c6d2027a5b7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  6e72f0a95b5e01a7504743aa941f69cb1fbef8b0 f6298a8ac3a4b78bbeae5f1d3dc5bc3c3812f0f3 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pushdest
  # obstore: pulldest
  ## pushing "A1" from main to pushdest
  pushing to pushdest
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files (+1 heads)
  remote: 1 new obsolescence markers
  remote: obsoleted 1 changesets
  ## post push state
  # obstore: main
  28b51eb45704506b5c603decd6bf7ac5e0f6a52f e5ea8f9c73143125d36658e90ef70c6d2027a5b7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  6e72f0a95b5e01a7504743aa941f69cb1fbef8b0 f6298a8ac3a4b78bbeae5f1d3dc5bc3c3812f0f3 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pushdest
  28b51eb45704506b5c603decd6bf7ac5e0f6a52f e5ea8f9c73143125d36658e90ef70c6d2027a5b7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pulldest
  ## pulling "e5ea8f9c7314" from main into pulldest
  pulling from main
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  1 new obsolescence markers
  obsoleted 1 changesets
  new changesets e5ea8f9c7314
  (run 'hg heads' to see heads, 'hg merge' to merge)
  ## post pull state
  # obstore: main
  28b51eb45704506b5c603decd6bf7ac5e0f6a52f e5ea8f9c73143125d36658e90ef70c6d2027a5b7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  6e72f0a95b5e01a7504743aa941f69cb1fbef8b0 f6298a8ac3a4b78bbeae5f1d3dc5bc3c3812f0f3 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pushdest
  28b51eb45704506b5c603decd6bf7ac5e0f6a52f e5ea8f9c73143125d36658e90ef70c6d2027a5b7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pulldest
  28b51eb45704506b5c603decd6bf7ac5e0f6a52f e5ea8f9c73143125d36658e90ef70c6d2027a5b7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
