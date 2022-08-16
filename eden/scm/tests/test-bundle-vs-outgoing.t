#chg-compatible
#debugruntest-compatible
  $ configure modernclient

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

  $ newclientrepo repo1
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

  $ hg up -r6a9ac14c32e0502be005fee0023b823698e3ce41
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ mkrev 4
  rev 4
  $ mkrev 5
  rev 5

merge first branch to second branch

  $ hg up -C -r'max(desc(rev))'
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

  $ hg up -ree67ca2f52ac8c7904cc477b8cf04da764fea594
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ mkrev 8
  rev 8

the story so far

  $ hg log -G --template "{node}\n"
  @  5f52be4fcfe9ac3202b79e6beb8804d871b98e10
  │
  │ o  de61c22a80e9fbe65e3f207212eb55d9c56e491b
  │ │
  │ o  a1e3db6b8fc126320c3bffd4c4b163c0d7f5038f
  ╭─┤
  o │  ee67ca2f52ac8c7904cc477b8cf04da764fea594
  │ │
  o │  4afa705929a3d9af58f4b035944e8ee600c7b571
  │ │
  │ o  478f191e53f84ddec1d358da2ed34eb796b3ac6f
  │ │
  │ o  c20e19c90a429c37cf2b85b397ebd3f3639ae890
  ├─╯
  o  6a9ac14c32e0502be005fee0023b823698e3ce41
  │
  o  6ae4cca4e39a527c4158d3b0fd73882b50e45484
  

check that "hg outgoing" really does the right thing

sanity check of outgoing: expect revs 4 5 6 7 8

  $ hg push -q -r 'desc(3)' --to book --create

test bundle (destination repo): expect 5 revisions

this should bundle the same 5 revisions that outgoing reported, but it

actually bundles 7

  $ hg bundle foo.bundle test:repo1_server
  searching for changes
  5 changesets found

test bundle (base revision): expect 5 revisions

this should (and does) give exactly the same result as bundle

with a destination repo... i.e. it's wrong too

  $ hg bundle --base 478f191e53f84ddec1d358da2ed34eb796b3ac6f foo.bundle
  5 changesets found

  $ cd ..
