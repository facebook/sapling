Load commonly used test logic
  $ . "$TESTDIR/testutil"

  $ git init gitrepo
  Initialized empty Git repository in $TESTTMP/gitrepo/.git/
  $ cd gitrepo
  $ echo alpha > alpha
  $ git add alpha
  $ fn_git_commit -m 'add alpha'
  $ echo beta > beta
  $ git add beta
  $ fn_git_commit -m 'add beta'
  $ mkdir foo
  $ echo blah > foo/bar
  $ git add foo
  $ fn_git_commit -m 'add foo'
  $ git rm alpha
  rm 'alpha'
  $ fn_git_commit -m 'remove alpha'
  $ git rm foo/bar
  rm 'foo/bar'
  $ fn_git_commit -m 'remove foo/bar'
  $ ln -s beta betalink
  $ git add betalink
  $ fn_git_commit -m 'add symlink to beta'
replace symlink with file
  $ rm betalink
  $ echo betalink > betalink
  $ git add betalink
  $ fn_git_commit -m 'replace symlink with file'
replace file with symlink
  $ rm betalink
  $ ln -s beta betalink
  $ git add betalink
  $ fn_git_commit -m 'replace file with symlink'
  $ git rm betalink
  rm 'betalink'
  $ fn_git_commit -m 'remove betalink'
final manifest in git is just beta
  $ git ls-files
  beta
  $ git log --pretty=medium
  commit 5ee11eeae239d6a99df5a99901ec00ffafbcc46b
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:18 2007 +0000
  
      remove betalink
  
  commit 2c7b324faeccb1acf89c35b7ad38e7956f5705fa
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:17 2007 +0000
  
      replace file with symlink
  
  commit ff0478d2ecc2571d01eb6d406ac29e4e63e5d3d5
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:16 2007 +0000
  
      replace symlink with file
  
  commit 5492e6e410e42df527956be945286cd1ae45acb8
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:15 2007 +0000
  
      add symlink to beta
  
  commit b991de8952c482a7cd51162674ffff8474862218
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:14 2007 +0000
  
      remove foo/bar
  
  commit b0edaf0adac19392cf2867498b983bc5192b41dd
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:13 2007 +0000
  
      remove alpha
  
  commit f2d0d5bfa905e12dee728b509b96cf265bb6ee43
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:12 2007 +0000
  
      add foo
  
  commit 9497a4ee62e16ee641860d7677cdb2589ea15554
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:11 2007 +0000
  
      add beta
  
  commit 7eeab2ea75ec1ac0ff3d500b5b6f8a3447dd7c03
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:10 2007 +0000
  
      add alpha

  $ cd ..
  $ git init --bare gitrepo2
  Initialized empty Git repository in $TESTTMP/gitrepo2/

  $ hg clone gitrepo hgrepo | grep -v '^updating'
  importing git objects into hg
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd hgrepo
  $ hg log --graph | grep -v ': *master'
  @  changeset:   8:0995b8a0a943
  |  tag:         default/master
  |  tag:         tip
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:18 2007 +0000
  |  summary:     remove betalink
  |
  o  changeset:   7:a316d3a96c89
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:17 2007 +0000
  |  summary:     replace file with symlink
  |
  o  changeset:   6:1804acb71f3e
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:16 2007 +0000
  |  summary:     replace symlink with file
  |
  o  changeset:   5:e19c85becc87
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:15 2007 +0000
  |  summary:     add symlink to beta
  |
  o  changeset:   4:0d3086c3f8c3
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:14 2007 +0000
  |  summary:     remove foo/bar
  |
  o  changeset:   3:b2406125ef5c
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:13 2007 +0000
  |  summary:     remove alpha
  |
  o  changeset:   2:8b3b2f4b4158
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:12 2007 +0000
  |  summary:     add foo
  |
  o  changeset:   1:7fe02317c63d
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:11 2007 +0000
  |  summary:     add beta
  |
  o  changeset:   0:ff7a2f2d8d70
     user:        test <test@example.org>
     date:        Mon Jan 01 00:00:10 2007 +0000
     summary:     add alpha
  

make sure alpha is not in this manifest
  $ hg manifest -r 3
  beta
  foo/bar

make sure that only beta is in the manifest
  $ hg manifest
  beta

  $ hg gclear
  clearing out the git cache data
  $ hg push ../gitrepo2
  pushing to ../gitrepo2
  searching for changes
  adding objects
  added 9 commits with 8 trees and 5 blobs

  $ cd ..
  $ git --git-dir=gitrepo2 log --pretty=medium
  commit 5ee11eeae239d6a99df5a99901ec00ffafbcc46b
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:18 2007 +0000
  
      remove betalink
  
  commit 2c7b324faeccb1acf89c35b7ad38e7956f5705fa
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:17 2007 +0000
  
      replace file with symlink
  
  commit ff0478d2ecc2571d01eb6d406ac29e4e63e5d3d5
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:16 2007 +0000
  
      replace symlink with file
  
  commit 5492e6e410e42df527956be945286cd1ae45acb8
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:15 2007 +0000
  
      add symlink to beta
  
  commit b991de8952c482a7cd51162674ffff8474862218
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:14 2007 +0000
  
      remove foo/bar
  
  commit b0edaf0adac19392cf2867498b983bc5192b41dd
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:13 2007 +0000
  
      remove alpha
  
  commit f2d0d5bfa905e12dee728b509b96cf265bb6ee43
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:12 2007 +0000
  
      add foo
  
  commit 9497a4ee62e16ee641860d7677cdb2589ea15554
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:11 2007 +0000
  
      add beta
  
  commit 7eeab2ea75ec1ac0ff3d500b5b6f8a3447dd7c03
  Author: test <test@example.org>
  Date:   Mon Jan 1 00:00:10 2007 +0000
  
      add alpha
