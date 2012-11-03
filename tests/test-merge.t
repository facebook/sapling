Load commonly used test logic
  $ . "$TESTDIR/testutil"

  $ git init gitrepo
  Initialized empty Git repository in $TESTTMP/gitrepo/.git/
  $ cd gitrepo
  $ echo alpha > alpha
  $ git add alpha
  $ fn_git_commit -m 'add alpha'

  $ git checkout -b beta 2>&1 | sed s/\'/\"/g
  Switched to a new branch "beta"
  $ echo beta > beta
  $ git add beta
  $ fn_git_commit -m 'add beta'

  $ git checkout master 2>&1 | sed s/\'/\"/g
  Switched to branch "master"
  $ echo gamma > gamma
  $ git add gamma
  $ fn_git_commit -m 'add gamma'

clean merge
  $ git merge beta | sed "s/the '//;s/' strategy//" | sed 's/^Merge.*recursive.*$/Merge successful/' | sed 's/files/file/;s/insertions/insertion/;s/, 0 deletions.*//' | sed 's/|  */| /'
  Merge successful
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

  $ echo % clear the cache to be sure it is regenerated correctly
  % clear the cache to be sure it is regenerated correctly
  $ hg gclear
  clearing out the git cache data
  $ hg push ../gitrepo2
  pushing to ../gitrepo2
  searching for changes

  $ cd ..
  $ echo % git log in repo pushed from hg
  % git log in repo pushed from hg
  $ git --git-dir=gitrepo2 log --pretty=medium master | sed 's/\.\.\.//g'
  commit 5806851511aaf3bfe813ae3a86c5027165fa9b96
  Merge: e5023f9 9497a4e
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:12 2007 +0000
  
      Merge branch 'beta'
  
  commit e5023f9e5cb24fdcec7b6c127cec45d8888e35a9
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:12 2007 +0000
  
      add gamma
  
  commit 9497a4ee62e16ee641860d7677cdb2589ea15554
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:11 2007 +0000
  
      add beta
  
  commit 7eeab2ea75ec1ac0ff3d500b5b6f8a3447dd7c03
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:10 2007 +0000
  
      add alpha
  $ git --git-dir=gitrepo2 log --pretty=medium beta | sed 's/\.\.\.//g'
  commit 9497a4ee62e16ee641860d7677cdb2589ea15554
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:11 2007 +0000
  
      add beta
  
  commit 7eeab2ea75ec1ac0ff3d500b5b6f8a3447dd7c03
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:10 2007 +0000
  
      add alpha
