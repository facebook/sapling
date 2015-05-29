  $ cat <<EOF > merge
  > import sys, os
  > 
  > try:
  >     import msvcrt
  >     msvcrt.setmode(sys.stdout.fileno(), os.O_BINARY)
  >     msvcrt.setmode(sys.stderr.fileno(), os.O_BINARY)
  > except ImportError:
  >     pass
  > 
  > print "merging for", os.path.basename(sys.argv[1])
  > EOF
  $ HGMERGE="python ../merge"; export HGMERGE

  $ hg init t
  $ cd t
  $ echo This is file a1 > a
  $ hg add a
  $ hg commit -m "commit #0"
  $ echo This is file b1 > b
  $ hg add b
  $ hg commit -m "commit #1"

  $ hg update 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

Test interrupted updates by exploiting our non-handling of directory collisions

  $ mkdir b
  $ hg up
  abort: *: '$TESTTMP/t/b' (glob)
  [255]
  $ hg ci
  abort: last update was interrupted
  (use 'hg update' to get a consistent checkout)
  [255]
  $ hg sum
  parent: 0:538afb845929 
   commit #0
  branch: default
  commit: (interrupted update)
  update: 1 new changesets (update)
  phases: 2 draft
  $ rmdir b
  $ hg up
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg sum
  parent: 1:b8bb4a988f25 tip
   commit #1
  branch: default
  commit: (clean)
  update: (current)
  phases: 2 draft

Prepare a basic merge

  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo This is file c1 > c
  $ hg add c
  $ hg commit -m "commit #2"
  created new head
  $ echo This is file b1 > b
no merges expected
  $ hg merge -P 1
  changeset:   1:b8bb4a988f25
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit #1
  
  $ hg merge 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg diff --nodates
  diff -r 49035e18a8e6 b
  --- /dev/null
  +++ b/b
  @@ -0,0 +1,1 @@
  +This is file b1
  $ hg status
  M b
  $ cd ..; rm -r t

  $ hg init t
  $ cd t
  $ echo This is file a1 > a
  $ hg add a
  $ hg commit -m "commit #0"
  $ echo This is file b1 > b
  $ hg add b
  $ hg commit -m "commit #1"

  $ hg update 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo This is file c1 > c
  $ hg add c
  $ hg commit -m "commit #2"
  created new head
  $ echo This is file b2 > b
merge should fail
  $ hg merge 1
  b: untracked file differs
  abort: untracked files in working directory differ from files in requested revision
  [255]
merge of b expected
  $ hg merge -f 1
  merging b
  merging for b
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg diff --nodates
  diff -r 49035e18a8e6 b
  --- /dev/null
  +++ b/b
  @@ -0,0 +1,1 @@
  +This is file b2
  $ hg status
  M b
  $ cd ..; rm -r t

  $ hg init t
  $ cd t
  $ echo This is file a1 > a
  $ hg add a
  $ hg commit -m "commit #0"
  $ echo This is file b1 > b
  $ hg add b
  $ hg commit -m "commit #1"
  $ echo This is file b22 > b
  $ hg commit -m "commit #2"
  $ hg update 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo This is file c1 > c
  $ hg add c
  $ hg commit -m "commit #3"
  created new head

Contents of b should be "this is file b1"
  $ cat b
  This is file b1

  $ echo This is file b22 > b
merge fails
  $ hg merge 2
  abort: uncommitted changes
  (use 'hg status' to list changes)
  [255]
merge expected!
  $ hg merge -f 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg diff --nodates
  diff -r 85de557015a8 b
  --- a/b
  +++ b/b
  @@ -1,1 +1,1 @@
  -This is file b1
  +This is file b22
  $ hg status
  M b
  $ cd ..; rm -r t

  $ hg init t
  $ cd t
  $ echo This is file a1 > a
  $ hg add a
  $ hg commit -m "commit #0"
  $ echo This is file b1 > b
  $ hg add b
  $ hg commit -m "commit #1"
  $ echo This is file b22 > b
  $ hg commit -m "commit #2"
  $ hg update 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo This is file c1 > c
  $ hg add c
  $ hg commit -m "commit #3"
  created new head
  $ echo This is file b33 > b
merge of b should fail
  $ hg merge 2
  abort: uncommitted changes
  (use 'hg status' to list changes)
  [255]
merge of b expected
  $ hg merge -f 2
  merging b
  merging for b
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg diff --nodates
  diff -r 85de557015a8 b
  --- a/b
  +++ b/b
  @@ -1,1 +1,1 @@
  -This is file b1
  +This is file b33
  $ hg status
  M b

Test for issue2364

  $ hg up -qC .
  $ hg rm b
  $ hg ci -md
  $ hg revert -r -2 b
  $ hg up -q -- -2

  $ cd ..
