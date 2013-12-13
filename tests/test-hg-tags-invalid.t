Load commonly used test logic
  $ . "$TESTDIR/testutil"

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
  $ hg co master
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ fn_hg_tag alph#a
  $ fn_hg_tag bet*a
  $ hg push
  pushing to $TESTTMP/gitrepo
  Skipping export of tag bet*a because it has invalid name as a git refname.
  searching for changes
  adding objects
  added 2 commits with 2 trees and 2 blobs
  updating reference refs/heads/master
  adding reference refs/tags/alph#a

  $ hg log --graph | egrep -v ': *(not-master|master)'
  @  changeset:   2:e72bdd9ef5c0
  |  tag:         default/master
  |  tag:         tip
  |  user:        test
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     Added tag bet*a for changeset 432ce25d86bc
  |
  o  changeset:   1:432ce25d86bc
  |  tag:         bet*a
  |  user:        test
  |  date:        Mon Jan 01 00:00:11 2007 +0000
  |  summary:     Added tag alph#a for changeset 3442585be8a6
  |
  o  changeset:   0:3442585be8a6
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

  $ cd ..
  $ hg clone gitrepo hgrepo2 | grep -v '^updating'
  importing git objects into hg
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R hgrepo2 log --graph | egrep -v ': *(not-master|master)'
  @  changeset:   2:e72bdd9ef5c0
  |  tag:         default/master
  |  tag:         tip
  |  user:        test
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     Added tag bet*a for changeset 432ce25d86bc
  |
  o  changeset:   1:432ce25d86bc
  |  tag:         bet*a
  |  user:        test
  |  date:        Mon Jan 01 00:00:11 2007 +0000
  |  summary:     Added tag alph#a for changeset 3442585be8a6
  |
  o  changeset:   0:3442585be8a6
     tag:         alph#a
     tag:         default/not-master
     user:        test <test@example.org>
     date:        Mon Jan 01 00:00:10 2007 +0000
     summary:     add alpha
  

the tag should be in .hgtags
  $ cat hgrepo2/.hgtags
  3442585be8a60c6cd476bbc4e45755339f2a23ef alph#a
  432ce25d86bc4281747aa42e27b473b992e2b0b9 bet*a
