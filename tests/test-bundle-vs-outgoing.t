this structure seems to tickle a bug in bundle's search for
changesets, so first we have to recreate it

o  8
|
| o  7
| |
| o  6
|/|
o |  5
| |
o |  4
| |
| o  3
| |
| o  2
|/
o  1
|
o  0

  $ mkrev()
  > {
  >     revno=$1
  >     echo "rev $revno"
  >     echo "rev $revno" > foo.txt
  >     hg -q ci -m"rev $revno"
  > }

setup test repo1

  $ hg init repo1
  $ cd repo1
  $ echo "rev 0" > foo.txt
  $ hg ci -Am"rev 0"
  adding foo.txt
  $ mkrev 1
  rev 1

first branch

  $ mkrev 2
  rev 2
  $ mkrev 3
  rev 3

back to rev 1 to create second branch

  $ hg up -r1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ mkrev 4
  rev 4
  $ mkrev 5
  rev 5

merge first branch to second branch

  $ hg up -C -r5
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ HGMERGE=internal:local hg merge
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ echo "merge rev 5, rev 3" > foo.txt
  $ hg ci -m"merge first branch to second branch"

one more commit following the merge

  $ mkrev 7
  rev 7

back to "second branch" to make another head

  $ hg up -r5
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ mkrev 8
  rev 8

the story so far

  $ hg log -G --template "{rev}\n"
  @  8
  |
  | o  7
  | |
  | o  6
  |/|
  o |  5
  | |
  o |  4
  | |
  | o  3
  | |
  | o  2
  |/
  o  1
  |
  o  0
  

check that "hg outgoing" really does the right thing

sanity check of outgoing: expect revs 4 5 6 7 8

  $ hg clone -r3 . ../repo2
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 4 changes to 1 files
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

this should (and does) report 5 outgoing revisions: 4 5 6 7 8

  $ hg outgoing --template "{rev}\n" ../repo2
  comparing with ../repo2
  searching for changes
  4
  5
  6
  7
  8

test bundle (destination repo): expect 5 revisions

this should bundle the same 5 revisions that outgoing reported, but it

actually bundles 7

  $ hg bundle foo.bundle ../repo2
  searching for changes
  5 changesets found

test bundle (base revision): expect 5 revisions

this should (and does) give exactly the same result as bundle

with a destination repo... i.e. it's wrong too

  $ hg bundle --base 3 foo.bundle
  5 changesets found

  $ cd ..
