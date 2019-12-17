Load commonly used test logic
  $ . "$TESTDIR/hggit/testutil"

  $ git init gitrepo
  Initialized empty Git repository in $TESTTMP/gitrepo/.git/
  $ cd gitrepo
  $ echo alpha > alpha
  $ git add alpha
  $ fn_git_commit -m "add alpha"
  $ git checkout -b not-master
  Switched to a new branch 'not-master'

  $ cd ..
  $ hg clone gitrepo hgrepo | grep -v '^updating'
  importing git objects into hg
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cd hgrepo
  $ hg co master | egrep -v '^\(activating bookmark master\)$'
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ fn_hg_tag alph#a
  $ fn_hg_tag bet*a
  $ fn_hg_tag 'gamm a'
  $ hg push
  pushing to $TESTTMP/gitrepo
  Skipping export of tag bet*a because it has invalid name as a git refname.
  searching for changes
  adding objects
  added 3 commits with 3 trees and 3 blobs
  updating reference refs/heads/master
  adding reference refs/tags/alph#a
  adding reference refs/tags/gamm_a

  $ hg log --graph
  @  changeset:   3:98151df7e752
  |  bookmark:    master
  |  tag:         default/master
  |  user:        test
  |  date:        Mon Jan 01 00:00:13 2007 +0000
  |  summary:     Added tag gamm a for changeset 44d87fefd1de
  |
  o  changeset:   2:44d87fefd1de
  |  tag:         gamm a
  |  user:        test
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     Added tag bet*a for changeset 8c962c6eae22
  |
  o  changeset:   1:8c962c6eae22
  |  tag:         bet*a
  |  user:        test
  |  date:        Mon Jan 01 00:00:11 2007 +0000
  |  summary:     Added tag alph#a for changeset 69982ec78c6d
  |
  o  changeset:   0:69982ec78c6d
     bookmark:    not-master
     tag:         alph#a
     tag:         default/not-master
     user:        test <test@example.org>
     date:        Mon Jan 01 00:00:10 2007 +0000
     summary:     add alpha
  

  $ cd ..
  $ cd gitrepo
git should have only the valid tag alph#a but have full commit log including the missing invalid bet*a tag commit
  $ git tag -l
  alph#a
  gamm_a

  $ cd ..
  $ hg clone gitrepo hgrepo2 | grep -v '^updating'
  importing git objects into hg
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R hgrepo2 log --graph
  @  changeset:   3:ca21cf0f93ef
  |  bookmark:    master
  |  tag:         default/master
  |  user:        test
  |  date:        Mon Jan 01 00:00:13 2007 +0000
  |  summary:     Added tag gamm a for changeset 44d87fefd1de
  |
  o  changeset:   2:1f92e71c01a9
  |  tag:         gamm_a
  |  user:        test
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     Added tag bet*a for changeset 8c962c6eae22
  |
  o  changeset:   1:3335035c29e5
  |  user:        test
  |  date:        Mon Jan 01 00:00:11 2007 +0000
  |  summary:     Added tag alph#a for changeset 69982ec78c6d
  |
  o  changeset:   0:69982ec78c6d
     bookmark:    not-master
     tag:         alph#a
     tag:         default/not-master
     user:        test <test@example.org>
     date:        Mon Jan 01 00:00:10 2007 +0000
     summary:     add alpha
  

the tag should be in .hgtags
  $ cat hgrepo2/.hgtags
  69982ec78c6dd2f24b3b62f3e2baaa79ab48ed93 alph#a
  8c962c6eae22f6ff70da4c8558f906cd4928c9cb bet*a
  44d87fefd1de70576229afe93a47ca6a22fdec67 gamm a
