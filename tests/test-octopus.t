Load commonly used test logic
  $ . "$TESTDIR/testutil"

bail if the user does not have git command-line client
  $ "$TESTDIR/hghave" git || exit 80

bail if the user does not have dulwich
  $ python -c 'import dulwich, dulwich.repo' || exit 80

  $ GIT_AUTHOR_NAME='test'; export GIT_AUTHOR_NAME
  $ GIT_AUTHOR_EMAIL='test@example.org'; export GIT_AUTHOR_EMAIL
  $ GIT_AUTHOR_DATE="2007-01-01 00:00:00 +0000"; export GIT_AUTHOR_DATE
  $ GIT_COMMITTER_NAME="$GIT_AUTHOR_NAME"; export GIT_COMMITTER_NAME
  $ GIT_COMMITTER_EMAIL="$GIT_AUTHOR_EMAIL"; export GIT_COMMITTER_EMAIL
  $ GIT_COMMITTER_DATE="$GIT_AUTHOR_DATE"; export GIT_COMMITTER_DATE

  $ count=10
  $ commit()
  > {
  >     GIT_AUTHOR_DATE="2007-01-01 00:00:$count +0000"
  >     GIT_COMMITTER_DATE="$GIT_AUTHOR_DATE"
  >     git commit "$@" >/dev/null 2>/dev/null || echo "git commit error" | sed 's/0 deletions(-)//'
  >     count=`expr $count + 1`
  > }

  $ mkdir gitrepo
  $ cd gitrepo
  $ git init
  Initialized empty Git repository in $TESTTMP/gitrepo/.git/

  $ echo alpha > alpha
  $ git add alpha
  $ commit -m 'add alpha'

  $ git checkout -b branch1 2>&1 | sed s/\'/\"/g
  Switched to a new branch "branch1"
  $ echo beta > beta
  $ git add beta
  $ commit -m 'add beta'

  $ git checkout -b branch2 master 2>&1 | sed s/\'/\"/g
  Switched to a new branch "branch2"
  $ echo gamma > gamma
  $ git add gamma
  $ commit -m 'add gamma'

  $ git checkout master 2>&1 | sed s/\'/\"/g
  Switched to branch "master"
  $ echo delta > delta
  $ git add delta
  $ commit -m 'add delta'

  $ git merge branch1 branch2 | sed "s/the '//;s/' strategy//" | sed 's/^Merge.*octopus.*$/Merge successful/;s/, 0 deletions.*//'  | sed 's/|  */| /'
  Trying simple merge with branch1
  Trying simple merge with branch2
  Merge successful
   beta  | 1 +
   gamma | 1 +
   2 files changed, 2 insertions(+)
   create mode 100644 beta
   create mode 100644 gamma

  $ cd ..
  $ mkdir gitrepo2
  $ cd gitrepo2
  $ git init --bare
  Initialized empty Git repository in $TESTTMP/gitrepo2/

  $ cd ..
  $ hg clone gitrepo hgrepo | grep -v '^updating'
  importing git objects into hg
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd hgrepo
  $ hg log --graph --style compact | sed 's/\[.*\]//g'
  @    5:3,4   6523aa9f4775   2007-01-01 00:00 +0000   test
  |\     Merge branches 'branch1' and 'branch2'
  | |
  | o    4:1,2   7f6c791a169f   2007-01-01 00:00 +0000   test
  | |\     Merge branches 'branch1' and 'branch2'
  | | |
  o | |  3:0   1436150b86c2   2007-01-01 00:00 +0000   test
  | | |    add delta
  | | |
  +---o  2:0   37c124f2d0a0   2007-01-01 00:00 +0000   test
  | |      add gamma
  | |
  | o  1   7bcd915dc873   2007-01-01 00:00 +0000   test
  |/     add beta
  |
  o  0   3442585be8a6   2007-01-01 00:00 +0000   test
       add alpha
  

  $ hg gclear
  clearing out the git cache data
  $ hg push ../gitrepo2
  pushing to ../gitrepo2
  searching for changes

  $ cd ../gitrepo2
  $ git log --pretty=medium | sed s/\\.\\.\\.//g
  commit f0c7ec180419a130636d0c333fc34c1462cab4b5
  Merge: d8e22dd 9497a4e e5023f9
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:13 2007 +0000
  
      Merge branches 'branch1' and 'branch2'
  
  commit d8e22ddb015d06460ccbb4508d2184c12c8a7c4c
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:13 2007 +0000
  
      add delta
  
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

  $ cd ..
