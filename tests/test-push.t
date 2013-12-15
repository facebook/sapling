Load commonly used test logic
  $ . "$TESTDIR/testutil"

  $ git init gitrepo
  Initialized empty Git repository in $TESTTMP/gitrepo/.git/
  $ cd gitrepo
  $ echo alpha > alpha
  $ git add alpha
  $ fn_git_commit -m "add alpha"
  $ git checkout -b not-master 2>&1 | sed s/\'/\"/g
  Switched to a new branch "not-master"

  $ cd ..
  $ hg clone gitrepo hgrepo | grep -v '^updating'
  importing git objects into hg
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cd hgrepo
  $ echo beta > beta
  $ hg add beta
  $ fn_hg_commit -m 'add beta'


  $ echo gamma > gamma
  $ hg add gamma
  $ fn_hg_commit -m 'add gamma'

  $ hg book -r 1 beta
  $ hg push -r beta
  pushing to $TESTTMP/gitrepo
  searching for changes
  adding objects
  added 1 commits with 1 trees and 1 blobs
  adding reference refs/heads/beta

  $ cd ..

should have two different branches
  $ cd gitrepo
  $ git branch -v
    beta       cffa0e8 add beta
    master     7eeab2e add alpha
  * not-master 7eeab2e add alpha

some more work on master from git
  $ git checkout master 2>&1 | sed s/\'/\"/g
  Switched to branch "master"
  $ echo delta > delta
  $ git add delta
  $ fn_git_commit -m "add delta"
  $ git checkout not-master 2>&1 | sed s/\'/\"/g
  Switched to branch "not-master"

  $ cd ..

  $ cd hgrepo
this should fail
  $ hg push -r master
  pushing to $TESTTMP/gitrepo
  searching for changes
  abort: branch 'refs/heads/master' changed on the server, please pull and merge before pushing
  [255]

... even with -f
  $ hg push -fr master
  pushing to $TESTTMP/gitrepo
  searching for changes
  abort: branch 'refs/heads/master' changed on the server, please pull and merge before pushing
  [255]

  $ hg pull 2>&1 | grep -v 'divergent bookmark'
  pulling from $TESTTMP/gitrepo
  importing git objects into hg
  (run 'hg update' to get a working copy)
TODO shouldn't need to do this since we're (in theory) pushing master explicitly,
which should not implicitly also push the not-master ref.
  $ hg book not-master -r default/not-master --force
master and default/master should be diferent
  $ hg log -r master | grep -v ': *master'
  changeset:   2:72f56395749d
  user:        test
  date:        Mon Jan 01 00:00:12 2007 +0000
  summary:     add gamma
  
  $ hg log -r default/master | grep -v 'master@default'
  changeset:   3:1436150b86c2
  tag:         default/master
  tag:         tip
  parent:      0:3442585be8a6
  user:        test <test@example.org>
  date:        Mon Jan 01 00:00:13 2007 +0000
  summary:     add delta
  

this should also fail
  $ hg push -r master
  pushing to $TESTTMP/gitrepo
  searching for changes
  abort: pushing refs/heads/master overwrites 72f56395749d
  [255]

... but succeed with -f
  $ hg push -fr master
  pushing to $TESTTMP/gitrepo
  searching for changes
  adding objects
  added 1 commits with 1 trees and 1 blobs
  updating reference refs/heads/master

this should fail, no changes to push
The exit code for this was broken in Mercurial (incorrectly returning 0) until
issue3228 was fixed in 2.1
  $ hg push -r master && false
  pushing to $TESTTMP/gitrepo
  searching for changes
  no changes found
  [1]

  $ cd ..

Push empty Hg repo to empty Git repo (issue #58)
Since there aren't any changes, exit code 1 is expected in modern Mercurial.
However, since it varies between supported Mercurial versions, we need to
force it to consistency for now. (see issue3228, fixed in Mercurial 2.1)
  $ hg init hgrepo2
  $ git init -q --bare gitrepo2
  $ hg -R hgrepo2 push gitrepo2 && false
  pushing to gitrepo2
  searching for changes
  no changes found
  [1]
