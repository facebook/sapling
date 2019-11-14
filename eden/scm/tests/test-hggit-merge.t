Load commonly used test logic
  $ . "$TESTDIR/hggit/testutil"

  $ git init gitrepo
  Initialized empty Git repository in $TESTTMP/gitrepo/.git/
  $ cd gitrepo
  $ echo alpha > alpha
  $ git add alpha
  $ fn_git_commit -m 'add alpha'

  $ git checkout -b beta
  Switched to a new branch 'beta'
  $ echo beta > beta
  $ git add beta
  $ fn_git_commit -m 'add beta'

  $ git checkout master
  Switched to branch 'master'
  $ echo gamma > gamma
  $ git add gamma
  $ fn_git_commit -m 'add gamma'

clean merge
  $ git merge beta
  Merge made by the 'recursive' strategy.
   beta | 1 +
   1 file changed, 1 insertion(+)
   create mode 100644 beta

  $ cd ..
  $ git init --bare gitrepo2
  Initialized empty Git repository in $TESTTMP/gitrepo2/

  $ hg clone gitrepo hgrepo | grep -v '^updating'
  importing git objects into hg
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd hgrepo

clear the cache to be sure it is regenerated correctly
  $ hg gclear
  clearing out the git cache data
  $ hg push ../gitrepo2
  pushing to ../gitrepo2
  searching for changes
  adding objects
  added 4 commits with 4 trees and 3 blobs

  $ cd ..
git log in repo pushed from hg
  $ git --git-dir=gitrepo2 log --pretty=medium master
  commit fdbdf0eb28dbc846a66f8bf458c5aa8ebfc87412
  Merge: 3ab4bf1 dbed4f6
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:12 2007 +0000
  
      Merge branch 'beta'
  
  commit 3ab4bf1785d6dbdb82467bf09e6aa1450312968d
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:12 2007 +0000
  
      add gamma
  
  commit dbed4f6a8ff04d4d1f0a5ce79f9a07cf0f461d7f
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:11 2007 +0000
  
      add beta
  
  commit 205598a42833e532ad20d80414b8e3b85a65936e
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:10 2007 +0000
  
      add alpha
  $ git --git-dir=gitrepo2 log --pretty=medium beta
  commit dbed4f6a8ff04d4d1f0a5ce79f9a07cf0f461d7f
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:11 2007 +0000
  
      add beta
  
  commit 205598a42833e532ad20d80414b8e3b85a65936e
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:10 2007 +0000
  
      add alpha
