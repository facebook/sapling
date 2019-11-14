Load commonly used test logic
  $ . "$TESTDIR/hggit/testutil"

  $ git init gitrepo
  Initialized empty Git repository in $TESTTMP/gitrepo/.git/
  $ cd gitrepo
  $ echo alpha > alpha
  $ git add alpha
  $ fn_git_commit -m 'add alpha'

  $ git checkout -b branch1
  Switched to a new branch 'branch1'
  $ echo beta > beta
  $ git add beta
  $ fn_git_commit -m 'add beta'

  $ git checkout -b branch2 master
  Switched to a new branch 'branch2'
  $ echo gamma > gamma
  $ git add gamma
  $ fn_git_commit -m 'add gamma'

  $ git checkout -b branch3 master
  Switched to a new branch 'branch3'
  $ echo epsilon > epsilon
  $ git add epsilon
  $ fn_git_commit -m 'add epsilon'

  $ git checkout -b branch4 master
  Switched to a new branch 'branch4'
  $ echo zeta > zeta
  $ git add zeta
  $ fn_git_commit -m 'add zeta'

  $ git checkout master
  Switched to branch 'master'
  $ echo delta > delta
  $ git add delta
  $ fn_git_commit -m 'add delta'

  $ git merge branch1 branch2
  Trying simple merge with branch1
  Trying simple merge with branch2
  Merge made by the 'octopus' strategy.
   beta  | 1 +
   gamma | 1 +
   2 files changed, 2 insertions(+)
   create mode 100644 beta
   create mode 100644 gamma

  $ git merge branch3 branch4
  Trying simple merge with branch3
  Trying simple merge with branch4
  Merge made by the 'octopus' strategy.
   epsilon | 1 +
   zeta    | 1 +
   2 files changed, 2 insertions(+)
   create mode 100644 epsilon
   create mode 100644 zeta

  $ cd ..
  $ git init --bare gitrepo2
  Initialized empty Git repository in $TESTTMP/gitrepo2/

  $ hg clone gitrepo hgrepo | grep -v '^updating'
  importing git objects into hg
  6 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd hgrepo
  $ hg log --graph --style compact
  @    9[default/master,tip][master]:7,8   49ab838a9c6d   2007-01-01 00:00 +0000   test
  |\     Merge branches 'branch3' and 'branch4'
  | |
  | o    8:3,4   772137582d44   2007-01-01 00:00 +0000   test
  | |\     Merge branches 'branch3' and 'branch4'
  | | |
  o | |    7:5,6   605318eb3cbf   2007-01-01 00:00 +0000   test
  |\ \ \     Merge branches 'branch1' and 'branch2'
  | | | |
  | o | |    6:1,2   c37d2773086b   2007-01-01 00:00 +0000   test
  | |\ \ \     Merge branches 'branch1' and 'branch2'
  | | | | |
  o | | | |  5:0   e70767a0294a   2007-01-01 00:00 +0000   test
  | | | | |    add delta
  | | | | |
  +-------o  4[default/branch4][branch4]:0   8b150650bbba   2007-01-01 00:00 +0000   test
  | | | |      add zeta
  | | | |
  +-----o  3[default/branch3][branch3]:0   b869fdf3e852   2007-01-01 00:00 +0000   test
  | | |      add epsilon
  | | |
  +---o  2[default/branch2][branch2]:0   328de8a94600   2007-01-01 00:00 +0000   test
  | |      add gamma
  | |
  | o  1[default/branch1][branch1]   3bb02b6794dd   2007-01-01 00:00 +0000   test
  |/     add beta
  |
  o  0   69982ec78c6d   2007-01-01 00:00 +0000   test
       add alpha
  
  $ hg gverify -r 9
  verifying rev 49ab838a9c6d against git commit b32ff845df61df998206b630e4370a44f9b36845
  $ hg gverify -r 8
  abort: no git commit found for rev 772137582d44
  (if this is an octopus merge, verify against the last rev)
  [255]

  $ hg gclear
  clearing out the git cache data
  $ hg push ../gitrepo2
  pushing to ../gitrepo2
  searching for changes
  adding objects
  added 8 commits with 8 trees and 6 blobs
  $ cd ..

  $ git --git-dir=gitrepo2 log --pretty=medium
  commit 2359e57d541911d60d80549ad41462b220d10c65
  Merge: f37a7b7 7ceac1d 692cf8a
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:15 2007 +0000
  
      Merge branches 'branch3' and 'branch4'
  
  commit f37a7b7b4969612fd5ab85b6d31d6465c25fef0b
  Merge: 47293d4 dbed4f6 3ab4bf1
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:15 2007 +0000
  
      Merge branches 'branch1' and 'branch2'
  
  commit 47293d46a21e55863c4a47f168731a2b9f95712b
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:15 2007 +0000
  
      add delta
  
  commit 692cf8ab35262a87694759a7668700632ca52c47
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:14 2007 +0000
  
      add zeta
  
  commit 7ceac1da981d4d67a88c662cc1c27e5e40c95884
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:13 2007 +0000
  
      add epsilon
  
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
