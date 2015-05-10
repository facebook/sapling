  $ HGMERGE=true; export HGMERGE

  $ hg init r1
  $ cd r1
  $ echo a > a
  $ hg addremove
  adding a
  $ hg commit -m "1"

  $ hg clone . ../r2
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ../r2
  $ hg up
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo abc > a
  $ hg diff --nodates
  diff -r c19d34741b0a a
  --- a/a
  +++ b/a
  @@ -1,1 +1,1 @@
  -a
  +abc

  $ cd ../r1
  $ echo b > b
  $ echo a2 > a
  $ hg addremove
  adding b
  $ hg commit -m "2"

  $ cd ../r2
  $ hg -q pull ../r1
  $ hg status
  M a
  $ hg parents
  changeset:   0:c19d34741b0a
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1
  
  $ hg --debug up
    searching for copies back to rev 1
    unmatched files in other:
     b
  resolving manifests
   branchmerge: False, force: False, partial: False
   ancestor: c19d34741b0a, local: c19d34741b0a+, remote: 1e71731e6fbb
   preserving a for resolve of a
   b: remote created -> g
  getting b
   a: versions differ -> m
  picked tool 'true' for a (binary False symlink False)
  merging a
  my a@c19d34741b0a+ other a@1e71731e6fbb ancestor a@c19d34741b0a
  launching merge tool: true *$TESTTMP/r2/a* * (glob)
  merge tool returned: 0
  1 files updated, 1 files merged, 0 files removed, 0 files unresolved
  $ hg parents
  changeset:   1:1e71731e6fbb
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2
  
  $ hg --debug up 0
  resolving manifests
   branchmerge: False, force: False, partial: False
   ancestor: 1e71731e6fbb, local: 1e71731e6fbb+, remote: c19d34741b0a
   preserving a for resolve of a
   b: other deleted -> r
  removing b
   a: versions differ -> m
  picked tool 'true' for a (binary False symlink False)
  merging a
  my a@1e71731e6fbb+ other a@c19d34741b0a ancestor a@1e71731e6fbb
  launching merge tool: true *$TESTTMP/r2/a* * (glob)
  merge tool returned: 0
  0 files updated, 1 files merged, 1 files removed, 0 files unresolved
  $ hg parents
  changeset:   0:c19d34741b0a
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1
  
  $ hg parents
  changeset:   0:c19d34741b0a
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1
  
  $ hg --debug up
    searching for copies back to rev 1
    unmatched files in other:
     b
  resolving manifests
   branchmerge: False, force: False, partial: False
   ancestor: c19d34741b0a, local: c19d34741b0a+, remote: 1e71731e6fbb
   preserving a for resolve of a
   b: remote created -> g
  getting b
   a: versions differ -> m
  picked tool 'true' for a (binary False symlink False)
  merging a
  my a@c19d34741b0a+ other a@1e71731e6fbb ancestor a@c19d34741b0a
  launching merge tool: true *$TESTTMP/r2/a* * (glob)
  merge tool returned: 0
  1 files updated, 1 files merged, 0 files removed, 0 files unresolved
  $ hg parents
  changeset:   1:1e71731e6fbb
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2
  
  $ hg -v history
  changeset:   1:1e71731e6fbb
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       a b
  description:
  2
  
  
  changeset:   0:c19d34741b0a
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       a
  description:
  1
  
  
  $ hg diff --nodates
  diff -r 1e71731e6fbb a
  --- a/a
  +++ b/a
  @@ -1,1 +1,1 @@
  -a2
  +abc


create a second head

  $ cd ../r1
  $ hg up 0
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo b2 > b
  $ echo a3 > a
  $ hg addremove
  adding b
  $ hg commit -m "3"
  created new head

  $ cd ../r2
  $ hg -q pull ../r1
  $ hg status
  M a
  $ hg parents
  changeset:   1:1e71731e6fbb
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2
  
  $ hg --debug up
  abort: uncommitted changes
  (commit and merge, or update --clean to discard changes)
  [255]

test conflicting untracked files

  $ hg up -qC 0
  $ echo untracked > b
  $ hg st
  ? b
  $ hg up 1
  b: untracked file differs
  abort: untracked files in working directory differ from files in requested revision
  [255]
  $ rm b

test conflicting untracked ignored file

  $ hg up -qC 0
  $ echo ignored > .hgignore
  $ hg add .hgignore
  $ hg ci -m 'add .hgignore'
  created new head
  $ echo ignored > ignored
  $ hg add ignored
  $ hg ci -m 'add ignored file'

  $ hg up -q 'desc("add .hgignore")'
  $ echo untracked > ignored
  $ hg st
  $ hg up 'desc("add ignored file")'
  ignored: untracked file differs
  abort: untracked files in working directory differ from files in requested revision
  [255]

test a local add

  $ cd ..
  $ hg init a
  $ hg init b
  $ echo a > a/a
  $ echo a > b/a
  $ hg --cwd a commit -A -m a
  adding a
  $ cd b
  $ hg add a
  $ hg pull -u ../a
  pulling from ../a
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg st

  $ cd ..
