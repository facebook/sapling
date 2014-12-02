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
  @  changeset:   8:c5d1976ab12c
  |  tag:         default/master
  |  tag:         tip
  |  user:        test >test@example.com>
  |  date:        Mon Jan 01 00:00:18 2007 +0000
  |  summary:     add theta
  |
  o  changeset:   7:0e2fb4d21667
  |  user:        test < test@example.com >
  |  date:        Mon Jan 01 00:00:17 2007 +0000
  |  summary:     add eta
  |
  o  changeset:   6:faa3aae96199
  |  user:        test
  |  date:        Mon Jan 01 00:00:16 2007 +0000
  |  summary:     add zeta
  |
  o  changeset:   5:2cf6ad5a1afc
  |  user:        name <test@example.com
  |  date:        Mon Jan 01 00:00:15 2007 +0000
  |  summary:     add epsilon
  |
  o  changeset:   4:3131b2a71396
  |  user:        name<test@example.com>
  |  date:        Mon Jan 01 00:00:14 2007 +0000
  |  summary:     add delta
  |
  o  changeset:   3:6b854d65d0d6
  |  user:        <test@example.com>
  |  date:        Mon Jan 01 00:00:13 2007 +0000
  |  summary:     add gamma
  |
  o  changeset:   2:46303c652e79
  |  user:        test <test@example.com> (comment)
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     modify beta
  |
  o  changeset:   1:47580592d3d6
  |  user:        test
  |  date:        Mon Jan 01 00:00:11 2007 +0000
  |  summary:     add beta
  |
  o  changeset:   0:ff7a2f2d8d70
     tag:         default/not-master
     user:        test <test@example.org>
     date:        Mon Jan 01 00:00:10 2007 +0000
     summary:     add alpha
  

  $ cd ..
  $ hg clone gitrepo hgrepo2 | grep -v '^updating'
  importing git objects into hg
  8 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R hgrepo2 log --graph | egrep -v ': *(not-master|master)'
  @  changeset:   8:1fbf3aa91221
  |  tag:         default/master
  |  tag:         tip
  |  user:        test ?test@example.com <test ?test@example.com>
  |  date:        Mon Jan 01 00:00:18 2007 +0000
  |  summary:     add theta
  |
  o  changeset:   7:20310508f06d
  |  user:        test <test@example.com>
  |  date:        Mon Jan 01 00:00:17 2007 +0000
  |  summary:     add eta
  |
  o  changeset:   6:e3d81af8a8c1
  |  user:        test
  |  date:        Mon Jan 01 00:00:16 2007 +0000
  |  summary:     add zeta
  |
  o  changeset:   5:78f609fd208f
  |  user:        name <test@example.com>
  |  date:        Mon Jan 01 00:00:15 2007 +0000
  |  summary:     add epsilon
  |
  o  changeset:   4:42fa61d57718
  |  user:        name <test@example.com>
  |  date:        Mon Jan 01 00:00:14 2007 +0000
  |  summary:     add delta
  |
  o  changeset:   3:6b854d65d0d6
  |  user:        <test@example.com>
  |  date:        Mon Jan 01 00:00:13 2007 +0000
  |  summary:     add gamma
  |
  o  changeset:   2:46303c652e79
  |  user:        test <test@example.com> (comment)
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     modify beta
  |
  o  changeset:   1:47580592d3d6
  |  user:        test
  |  date:        Mon Jan 01 00:00:11 2007 +0000
  |  summary:     add beta
  |
  o  changeset:   0:ff7a2f2d8d70
     tag:         default/not-master
     user:        test <test@example.org>
     date:        Mon Jan 01 00:00:10 2007 +0000
     summary:     add alpha
  
  $ git --git-dir=gitrepo/.git log --pretty=medium master
  commit 2fe60ba69727981e6ede78be70354c3a9e30e21d
  Author: test ?test@example.com <test ?test@example.com>
  Date:   Mon Jan 1 00:00:18 2007 +0000
  
      add theta
  
  commit 9f2f7cafdbf2e467928db98de8275141001d3081
  Author: test <test@example.com>
  Date:   Mon Jan 1 00:00:17 2007 +0000
  
      add eta
  
  commit 172a6f8d8064d73dff7013e395a9fe3cfc3ff807
  Author: test <none@none>
  Date:   Mon Jan 1 00:00:16 2007 +0000
  
      add zeta
  
  commit 71badb8e343a7da391a9b5d98909fbd2ca7d78f2
  Author: name <test@example.com>
  Date:   Mon Jan 1 00:00:15 2007 +0000
  
      add epsilon
  
  commit 9a9ae7b7f310d4a1a3e732a747ca26f06934f8d8
  Author: name <test@example.com>
  Date:   Mon Jan 1 00:00:14 2007 +0000
  
      add delta
  
  commit e4149a32e81e380193f59aa8773349201b8ed7f7
  Author:  <test@example.com>
  Date:   Mon Jan 1 00:00:13 2007 +0000
  
      add gamma
  
  commit fae95aef5889a80103c2fbd5d14ff6eb8c9daf93
  Author: test ext:(%20%28comment%29) <test@example.com>
  Date:   Mon Jan 1 00:00:12 2007 +0000
  
      modify beta
  
  commit 0f378ab6c2c6b5514bd873d3faf8ac4b8095b001
  Author: test <none@none>
  Date:   Mon Jan 1 00:00:11 2007 +0000
  
      add beta
  
  commit 7eeab2ea75ec1ac0ff3d500b5b6f8a3447dd7c03
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:10 2007 +0000
  
      add alpha
