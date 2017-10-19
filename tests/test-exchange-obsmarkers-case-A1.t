============================================
Testing obsolescence markers push: Cases A.1
============================================

Mercurial pushes obsolescences markers relevant to the "pushed-set", the set of
all changesets that requested to be "in sync" after the push (even if they are
already on both side).

This test belongs to a series of tests checking such set is properly computed
and applied. This does not tests "obsmarkers" discovery capabilities.

Category A: simple cases
TestCase 1: pushing a single head
Subcases:
# A.1.1 pushing a single head (2 variants)
# A.1.2 pushing multiple changesets into a single head (2 variants)

Case: A.1.1 pushing a single head
=================================
..
.. {{{
..     ⇠◔ A
..      |
..      ● O
.. }}}
..
.. Marker exists from:
..
..  * A
..
.. Commands run:
..
..  * hg push -r A
..  * hg push
..
.. Expected exchange:
..
..  * chain from A

Setup
-----

  $ . $TESTDIR/testlib/exchange-obsmarker-util.sh

initial

  $ setuprepos A.1.1
  creating test repo for test case A.1.1
  - pulldest
  - main
  - pushdest
  cd into `main` and proceed with env setup
  $ cd main
  $ mkcommit A
  $ hg debugobsolete aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa `getid 'desc(A)'`
  $ hg log -G
  @  f5bc6836db60 (draft): A
  |
  o  a9bdc8b26820 (public): O
  
  $ inspect_obsmarkers
  obsstore content
  ================
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa f5bc6836db60e308a17ba08bf050154ba9c4fad7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  $ cd ..
  $ cd ..

setup both variants

  $ cp -R A.1.1 A.1.1.a
  $ cp -R A.1.1 A.1.1.b

Variant a: push -r A
--------------------

  $ dotest A.1.1.a A
  ## Running testcase A.1.1.a
  # testing echange of "A" (f5bc6836db60)
  ## initial state
  # obstore: main
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa f5bc6836db60e308a17ba08bf050154ba9c4fad7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
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
  # obstore: pushdest
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa f5bc6836db60e308a17ba08bf050154ba9c4fad7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pulldest
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa f5bc6836db60e308a17ba08bf050154ba9c4fad7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}

Variant b: push
---------------

  $ dotest A.1.1.b
  ## Running testcase A.1.1.b
  ## initial state
  # obstore: main
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa f5bc6836db60e308a17ba08bf050154ba9c4fad7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pushdest
  # obstore: pulldest
  ## pushing from main to pushdest
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
  # obstore: pushdest
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa f5bc6836db60e308a17ba08bf050154ba9c4fad7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pulldest
  ## pulling from main into pulldest
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
  # obstore: pushdest
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa f5bc6836db60e308a17ba08bf050154ba9c4fad7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pulldest
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa f5bc6836db60e308a17ba08bf050154ba9c4fad7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}

A.1.2 pushing multiple changesets into a single head
====================================================

.. {{{
..      ◔ B
..      |
..     ⇠◔ A
..      |
..      ● O
.. }}}
..
.. Marker exist from:
..
..  * A
..
.. Command run:
..
..  * hg push -r B
..  * hg push
..
.. Expected exchange:
..
..  * chain from A

Setup
-----

initial

  $ setuprepos A.1.2
  creating test repo for test case A.1.2
  - pulldest
  - main
  - pushdest
  cd into `main` and proceed with env setup
  $ cd main
  $ mkcommit A
  $ mkcommit B
  $ hg log -G
  @  f6fbb35d8ac9 (draft): B
  |
  o  f5bc6836db60 (draft): A
  |
  o  a9bdc8b26820 (public): O
  
  $ hg debugobsolete aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa `getid 'desc(A)'`
  $ inspect_obsmarkers
  obsstore content
  ================
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa f5bc6836db60e308a17ba08bf050154ba9c4fad7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  $ cd ..
  $ cd ..

setup both variants

  $ cp -R A.1.2 A.1.2.a
  $ cp -R A.1.2 A.1.2.b

Variant a: push -r A
--------------------

  $ dotest A.1.2.a B
  ## Running testcase A.1.2.a
  # testing echange of "B" (f6fbb35d8ac9)
  ## initial state
  # obstore: main
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa f5bc6836db60e308a17ba08bf050154ba9c4fad7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pushdest
  # obstore: pulldest
  ## pushing "B" from main to pushdest
  pushing to pushdest
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 2 changesets with 2 changes to 2 files
  remote: 1 new obsolescence markers
  ## post push state
  # obstore: main
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa f5bc6836db60e308a17ba08bf050154ba9c4fad7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pushdest
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa f5bc6836db60e308a17ba08bf050154ba9c4fad7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pulldest
  ## pulling "f6fbb35d8ac9" from main into pulldest
  pulling from main
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  1 new obsolescence markers
  new changesets f5bc6836db60:f6fbb35d8ac9
  (run 'hg update' to get a working copy)
  ## post pull state
  # obstore: main
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa f5bc6836db60e308a17ba08bf050154ba9c4fad7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pushdest
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa f5bc6836db60e308a17ba08bf050154ba9c4fad7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pulldest
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa f5bc6836db60e308a17ba08bf050154ba9c4fad7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}

Variant b: push
---------------

  $ dotest A.1.2.b
  ## Running testcase A.1.2.b
  ## initial state
  # obstore: main
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa f5bc6836db60e308a17ba08bf050154ba9c4fad7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pushdest
  # obstore: pulldest
  ## pushing from main to pushdest
  pushing to pushdest
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 2 changesets with 2 changes to 2 files
  remote: 1 new obsolescence markers
  ## post push state
  # obstore: main
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa f5bc6836db60e308a17ba08bf050154ba9c4fad7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pushdest
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa f5bc6836db60e308a17ba08bf050154ba9c4fad7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pulldest
  ## pulling from main into pulldest
  pulling from main
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  1 new obsolescence markers
  new changesets f5bc6836db60:f6fbb35d8ac9
  (run 'hg update' to get a working copy)
  ## post pull state
  # obstore: main
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa f5bc6836db60e308a17ba08bf050154ba9c4fad7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pushdest
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa f5bc6836db60e308a17ba08bf050154ba9c4fad7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  # obstore: pulldest
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa f5bc6836db60e308a17ba08bf050154ba9c4fad7 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
