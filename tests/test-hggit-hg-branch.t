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
  $ hg mv alpha beta
  $ fn_hg_commit -m 'rename alpha to beta'
  $ hg push
  pushing to $TESTTMP/gitrepo
  searching for changes
  adding objects
  added 1 commits with 1 trees and 0 blobs
  updating reference refs/heads/master

  $ hg branch gamma | grep -v 'permanent and global'
  marked working directory as branch gamma
  $ fn_hg_commit -m 'started branch gamma'
  $ hg push
  pushing to $TESTTMP/gitrepo
  searching for changes
  adding objects
  added 1 commits with 1 trees and 0 blobs
  updating reference refs/heads/master

  $ hg log --graph
  @  changeset:   2:a305a33d1ab8
  |  branch:      gamma
  |  bookmark:    master
  |  tag:         default/master
  |  tag:         tip
  |  user:        test
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     started branch gamma
  |
  o  changeset:   1:e8aafc55ea94
  |  user:        test
  |  date:        Mon Jan 01 00:00:11 2007 +0000
  |  summary:     rename alpha to beta
  |
  o  changeset:   0:69982ec78c6d
     bookmark:    not-master
     tag:         default/not-master
     user:        test <test@example.org>
     date:        Mon Jan 01 00:00:10 2007 +0000
     summary:     add alpha
  

  $ cd ..
  $ hg clone gitrepo hgrepo2 | grep -v '^updating'
  importing git objects into hg
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R hgrepo2 log --graph
  o  changeset:   2:b9f45971f9de
  |  branch:      gamma
  |  bookmark:    master
  |  tag:         default/master
  |  tag:         tip
  |  user:        test
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     started branch gamma
  |
  @  changeset:   1:723be17884a5
  |  user:        test
  |  date:        Mon Jan 01 00:00:11 2007 +0000
  |  summary:     rename alpha to beta
  |
  o  changeset:   0:69982ec78c6d
     bookmark:    not-master
     tag:         default/not-master
     user:        test <test@example.org>
     date:        Mon Jan 01 00:00:10 2007 +0000
     summary:     add alpha
  
