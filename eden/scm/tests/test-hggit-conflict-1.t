Load commonly used test logic
  $ . "$TESTDIR/hggit/testutil"

  $ hg init hgrepo1
  $ cd hgrepo1
  $ echo A > afile
  $ hg add afile
  $ hg ci -m "origin"

  $ echo B > afile
  $ hg ci -m "A->B"

  $ hg up -r0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo C > afile
  $ hg ci -m "A->C"

  $ hg merge -r1
  merging afile
  warning: 1 conflicts while merging afile! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
resolve using first parent
  $ echo C > afile
  $ hg resolve -m afile | egrep -v 'no more unresolved files' || true
  $ hg ci -m "merge to C"

  $ hg log --graph --style compact
  @    :ea82b67264a1,7205e83b5a3f   6c53bc0f062f   1970-01-01 00:00 +0000   test
  |\     merge to C
  | |
  | o  :5d1a6b64f9d0   ea82b67264a1   1970-01-01 00:00 +0000   test
  | |    A->C
  | |
  o |     7205e83b5a3f   1970-01-01 00:00 +0000   test
  |/     A->B
  |
  o     5d1a6b64f9d0   1970-01-01 00:00 +0000   test
       origin
  

  $ cd ..

  $ git init --bare gitrepo
  Initialized empty Git repository in $TESTTMP/gitrepo/

  $ cd hgrepo1
  $ hg bookmark -r tip master
  $ hg push -r master ../gitrepo
  pushing to ../gitrepo
  searching for changes
  adding objects
  added 4 commits with 3 trees and 3 blobs
  $ cd ..

  $ hg clone gitrepo hgrepo2 | grep -v '^updating'
  importing git objects into hg
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
expect the same revision ids as above
  $ hg -R hgrepo2 log --graph --style compact
  @    [master]:8ec5b459b86e,47fc555571b8   b70d5f2ec3c4   1970-01-01 00:00 +0000   test
  |\     merge to C
  | |
  | o  :fd5eb788c3a1   47fc555571b8   1970-01-01 00:00 +0000   test
  | |    A->B
  | |
  o |     8ec5b459b86e   1970-01-01 00:00 +0000   test
  |/     A->C
  |
  o     fd5eb788c3a1   1970-01-01 00:00 +0000   test
       origin
  
