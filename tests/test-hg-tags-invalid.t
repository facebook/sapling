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

  $ hg log --graph | egrep -v ': *(not-master|master)'
  @  changeset:   3:0950ab44ea23
  |  tag:         default/master
  |  tag:         tip
  |  user:        test
  |  date:        Mon Jan 01 00:00:13 2007 +0000
  |  summary:     Added tag gamm a for changeset 0b27ab2b3df6
  |
  o  changeset:   2:0b27ab2b3df6
  |  tag:         gamm a
  |  user:        test
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     Added tag bet*a for changeset 491ceeb1b0f1
  |
  o  changeset:   1:491ceeb1b0f1
  |  tag:         bet*a
  |  user:        test
  |  date:        Mon Jan 01 00:00:11 2007 +0000
  |  summary:     Added tag alph#a for changeset ff7a2f2d8d70
  |
  o  changeset:   0:ff7a2f2d8d70
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
  $ hg -R hgrepo2 log --graph | egrep -v ': *(not-master|master)'
  @  changeset:   3:0950ab44ea23
  |  tag:         default/master
  |  tag:         tip
  |  user:        test
  |  date:        Mon Jan 01 00:00:13 2007 +0000
  |  summary:     Added tag gamm a for changeset 0b27ab2b3df6
  |
  o  changeset:   2:0b27ab2b3df6
  |  tag:         gamm a
  |  tag:         gamm_a
  |  user:        test
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     Added tag bet*a for changeset 491ceeb1b0f1
  |
  o  changeset:   1:491ceeb1b0f1
  |  tag:         bet*a
  |  user:        test
  |  date:        Mon Jan 01 00:00:11 2007 +0000
  |  summary:     Added tag alph#a for changeset ff7a2f2d8d70
  |
  o  changeset:   0:ff7a2f2d8d70
     tag:         alph#a
     tag:         default/not-master
     user:        test <test@example.org>
     date:        Mon Jan 01 00:00:10 2007 +0000
     summary:     add alpha
  

the tag should be in .hgtags
  $ cat hgrepo2/.hgtags
  ff7a2f2d8d7099694ae1e8b03838d40575bebb63 alph#a
  491ceeb1b0f10d65d956dfcdd3470ac2bc2c96a8 bet*a
  0b27ab2b3df69c6f7defd7040b93e539136db5be gamm a
