Load commonly used test logic
  $ . "$TESTDIR/testutil"

TODO stop using this when we're 1.5 only
  $ filterhash="sed s/71414c4e3c6f/a31e374801c9/;s/698615204564/d93a72262a83/"
  $ filterhash="$filterhash;s/d93a72262a83/05aed681ccb3/"

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
  $ hg mv alpha beta
  $ fn_hg_commit -m 'rename alpha to beta'
  $ hg push
  pushing to $TESTTMP/gitrepo
  searching for changes

  $ hg branch gamma | grep -v 'permanent and global'
  marked working directory as branch gamma
  $ fn_hg_commit -m 'started branch gamma'
  $ hg push
  pushing to $TESTTMP/gitrepo
  searching for changes

  $ hg log --graph | $filterhash | egrep -v ': *(not-master|master)'
  @  changeset:   2:05aed681ccb3
  |  branch:      gamma
  |  tag:         default/master
  |  tag:         tip
  |  user:        test
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     started branch gamma
  |
  o  changeset:   1:a31e374801c9
  |  user:        test
  |  date:        Mon Jan 01 00:00:11 2007 +0000
  |  summary:     rename alpha to beta
  |
  o  changeset:   0:3442585be8a6
     tag:         default/not-master
     user:        test <test@example.org>
     date:        Mon Jan 01 00:00:10 2007 +0000
     summary:     add alpha
  

  $ cd ..
  $ hg clone gitrepo hgrepo2 | grep -v '^updating'
  importing git objects into hg
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd hgrepo2
  $ hg log --graph | $filterhash | egrep -v ': *(not-master|master)'
  o  changeset:   2:05aed681ccb3
  |  branch:      gamma
  |  tag:         default/master
  |  tag:         tip
  |  user:        test
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     started branch gamma
  |
  @  changeset:   1:a31e374801c9
  |  user:        test
  |  date:        Mon Jan 01 00:00:11 2007 +0000
  |  summary:     rename alpha to beta
  |
  o  changeset:   0:3442585be8a6
     tag:         default/not-master
     user:        test <test@example.org>
     date:        Mon Jan 01 00:00:10 2007 +0000
     summary:     add alpha
  

  $ cd ..
