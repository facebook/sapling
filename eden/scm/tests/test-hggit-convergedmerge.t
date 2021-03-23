Load commonly used test logic
  $ . "$TESTDIR/hggit/testutil"

  $ hg init hgrepo1
  $ cd hgrepo1
  $ echo A > afile
  $ hg add afile
  $ hg ci -m "origin"

  $ echo B > afile
  $ hg ci -m "A->B"

  $ echo C > afile
  $ hg ci -m "B->C"

  $ hg up -r'desc(origin)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo C > afile
  $ hg ci -m "A->C"

  $ hg merge -r0dbe4ac1a7586d1642016eea4781390285b7b536
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m "merge"

  $ hg log --graph --style compact
  @       eaa21d002113   1970-01-01 00:00 +0000   test
  ├─╮    merge
  │ │
  │ o     ea82b67264a1   1970-01-01 00:00 +0000   test
  │ │    A->C
  │ │
  o │     0dbe4ac1a758   1970-01-01 00:00 +0000   test
  │ │    B->C
  │ │
  o │     7205e83b5a3f   1970-01-01 00:00 +0000   test
  ├─╯    A->B
  │
  o     5d1a6b64f9d0   1970-01-01 00:00 +0000   test
       origin
  

  $ cd ..

  $ git init -q --bare gitrepo

  $ cd hgrepo1
  $ hg bookmark -r'desc(merge)' master
  $ hg push -r master ../gitrepo
  pushing to ../gitrepo
  searching for changes
  adding objects
  added 5 commits with 3 trees and 3 blobs
  $ cd ..

  $ hg clone gitrepo hgrepo2 | grep -v '^updating'
  importing git objects into hg
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
expect the same revision ids as above
  $ hg -R hgrepo2 log --graph --style compact
  @    [master]   b08a922386d5   1970-01-01 00:00 +0000   test
  ├─╮    merge
  │ │
  │ o     8bfd72bff163   1970-01-01 00:00 +0000   test
  │ │    B->C
  │ │
  │ o     47fc555571b8   1970-01-01 00:00 +0000   test
  │ │    A->B
  │ │
  o │     8ec5b459b86e   1970-01-01 00:00 +0000   test
  ├─╯    A->C
  │
  o     fd5eb788c3a1   1970-01-01 00:00 +0000   test
       origin
  
  $ hg -R hgrepo2 gverify
  verifying rev b08a922386d5 against git commit fb8c9e2afe5418cfff337eeed79fad5dd58826f0
