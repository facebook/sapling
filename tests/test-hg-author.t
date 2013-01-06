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
  $ echo beta > beta
  $ hg add beta
  $ fn_hg_commit -u "test" -m 'add beta'
  $ hg push
  pushing to $TESTTMP/gitrepo
  searching for changes
  adding objects
  added 1 commits with 1 trees and 1 blobs
  updating reference refs/heads/master

  $ echo gamma >> beta
  $ fn_hg_commit -u "test <test@example.com> (comment)" -m 'modify beta'
  $ hg push
  pushing to $TESTTMP/gitrepo
  searching for changes
  adding objects
  added 1 commits with 1 trees and 1 blobs
  updating reference refs/heads/master

  $ echo gamma > gamma
  $ hg add gamma
  $ fn_hg_commit -u "<test@example.com>" -m 'add gamma'
  $ hg push
  pushing to $TESTTMP/gitrepo
  searching for changes
  adding objects
  added 1 commits with 1 trees and 1 blobs
  updating reference refs/heads/master

  $ echo delta > delta
  $ hg add delta
  $ fn_hg_commit -u "name<test@example.com>" -m 'add delta'
  $ hg push
  pushing to $TESTTMP/gitrepo
  searching for changes
  adding objects
  added 1 commits with 1 trees and 1 blobs
  updating reference refs/heads/master

  $ echo epsilon > epsilon
  $ hg add epsilon
  $ fn_hg_commit -u "name <test@example.com" -m 'add epsilon'
  $ hg push
  pushing to $TESTTMP/gitrepo
  searching for changes
  adding objects
  added 1 commits with 1 trees and 1 blobs
  updating reference refs/heads/master

  $ echo zeta > zeta
  $ hg add zeta
  $ fn_hg_commit -u " test " -m 'add zeta'
  $ hg push
  pushing to $TESTTMP/gitrepo
  searching for changes
  adding objects
  added 1 commits with 1 trees and 1 blobs
  updating reference refs/heads/master

  $ echo eta > eta
  $ hg add eta
  $ fn_hg_commit -u "test < test@example.com >" -m 'add eta'
  $ hg push
  pushing to $TESTTMP/gitrepo
  searching for changes
  adding objects
  added 1 commits with 1 trees and 1 blobs
  updating reference refs/heads/master

  $ echo theta > theta
  $ hg add theta
  $ fn_hg_commit -u "test >test@example.com>" -m 'add theta'
  $ hg push
  pushing to $TESTTMP/gitrepo
  searching for changes
  adding objects
  added 1 commits with 1 trees and 1 blobs
  updating reference refs/heads/master

  $ hg log --graph | egrep -v ': *(not-master|master)'
  @  changeset:   8:d3c51ce68cfd
  |  tag:         default/master
  |  tag:         tip
  |  user:        test >test@example.com>
  |  date:        Mon Jan 01 00:00:18 2007 +0000
  |  summary:     add theta
  |
  o  changeset:   7:b90e988091a2
  |  user:        test < test@example.com >
  |  date:        Mon Jan 01 00:00:17 2007 +0000
  |  summary:     add eta
  |
  o  changeset:   6:7ede2f971cae
  |  user:        test
  |  date:        Mon Jan 01 00:00:16 2007 +0000
  |  summary:     add zeta
  |
  o  changeset:   5:1454a94056ec
  |  user:        name <test@example.com
  |  date:        Mon Jan 01 00:00:15 2007 +0000
  |  summary:     add epsilon
  |
  o  changeset:   4:a045fd599678
  |  user:        name<test@example.com>
  |  date:        Mon Jan 01 00:00:14 2007 +0000
  |  summary:     add delta
  |
  o  changeset:   3:8da3ab8b31d0
  |  user:        <test@example.com>
  |  date:        Mon Jan 01 00:00:13 2007 +0000
  |  summary:     add gamma
  |
  o  changeset:   2:92d33c0dd6e1
  |  user:        test <test@example.com> (comment)
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     modify beta
  |
  o  changeset:   1:0564f526fb0f
  |  user:        test
  |  date:        Mon Jan 01 00:00:11 2007 +0000
  |  summary:     add beta
  |
  o  changeset:   0:3442585be8a6
     tag:         default/not-master
     user:        test <test@example.org>
     date:        Mon Jan 01 00:00:10 2007 +0000
     summary:     add alpha
  

  $ cd ..
  $ hg clone gitrepo hgrepo2 | grep -v '^updating'
  importing git objects into hg
  8 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R hgrepo2 log --graph | egrep -v ': *(not-master|master)'
  @  changeset:   8:efec0270e295
  |  tag:         default/master
  |  tag:         tip
  |  user:        test ?test@example.com <test ?test@example.com>
  |  date:        Mon Jan 01 00:00:18 2007 +0000
  |  summary:     add theta
  |
  o  changeset:   7:8ab87d5066e4
  |  user:        test <test@example.com>
  |  date:        Mon Jan 01 00:00:17 2007 +0000
  |  summary:     add eta
  |
  o  changeset:   6:ff226cc916bd
  |  user:        test
  |  date:        Mon Jan 01 00:00:16 2007 +0000
  |  summary:     add zeta
  |
  o  changeset:   5:5f1557c62c53
  |  user:        name <test@example.com>
  |  date:        Mon Jan 01 00:00:15 2007 +0000
  |  summary:     add epsilon
  |
  o  changeset:   4:fc51727b28fe
  |  user:        name <test@example.com>
  |  date:        Mon Jan 01 00:00:14 2007 +0000
  |  summary:     add delta
  |
  o  changeset:   3:8da3ab8b31d0
  |  user:        <test@example.com>
  |  date:        Mon Jan 01 00:00:13 2007 +0000
  |  summary:     add gamma
  |
  o  changeset:   2:92d33c0dd6e1
  |  user:        test <test@example.com> (comment)
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     modify beta
  |
  o  changeset:   1:0564f526fb0f
  |  user:        test
  |  date:        Mon Jan 01 00:00:11 2007 +0000
  |  summary:     add beta
  |
  o  changeset:   0:3442585be8a6
     tag:         default/not-master
     user:        test <test@example.org>
     date:        Mon Jan 01 00:00:10 2007 +0000
     summary:     add alpha
  
  $ git --git-dir=gitrepo/.git log --pretty=medium master
  commit 1e03e913eca571b86ee06d3c1ddd795dde9ca917
  Author: test ?test@example.com <test ?test@example.com>
  Date:   Mon Jan 1 00:00:18 2007 +0000
  
      add theta
  
  commit 8c878c9764e96e67ed9f62b3f317d156bf71bc52
  Author: test <test@example.com>
  Date:   Mon Jan 1 00:00:17 2007 +0000
  
      add eta
  
  commit d21e26b48c6136340dd1212bb45ba0e9debb130c
  Author: test <none@none>
  Date:   Mon Jan 1 00:00:16 2007 +0000
  
      add zeta
  
  commit ee985f124d2f13ee8ad2a346a6d1b0ada8b0d491
  Author: name <test@example.com>
  Date:   Mon Jan 1 00:00:15 2007 +0000
  
      add epsilon
  
  commit d16592507ac83a6a633b90ca255f65e5d024f0bc
  Author: name <test@example.com>
  Date:   Mon Jan 1 00:00:14 2007 +0000
  
      add delta
  
  commit fee30180efc4943fb916de04fcf6a64b638d9325
  Author:  <test@example.com>
  Date:   Mon Jan 1 00:00:13 2007 +0000
  
      add gamma
  
  commit 2b9ec6a47b93191986a79eeb771e461c4508c7c4
  Author: test ext:(%20%28comment%29) <test@example.com>
  Date:   Mon Jan 1 00:00:12 2007 +0000
  
      modify beta
  
  commit cffa0e8d8ad5f284c69c898c0f3c1e32d078af8a
  Author: test <none@none>
  Date:   Mon Jan 1 00:00:11 2007 +0000
  
      add beta
  
  commit 7eeab2ea75ec1ac0ff3d500b5b6f8a3447dd7c03
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:10 2007 +0000
  
      add alpha
