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
  updating: b 1/2 files (50.00%)
   a: versions differ -> m
  updating: a 2/2 files (100.00%)
  picked tool 'true' for a (binary False symlink False)
  merging a
  my a@c19d34741b0a+ other a@1e71731e6fbb ancestor a@c19d34741b0a
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
  updating: b 1/2 files (50.00%)
   a: versions differ -> m
  updating: a 2/2 files (100.00%)
  picked tool 'true' for a (binary False symlink False)
  merging a
  my a@1e71731e6fbb+ other a@c19d34741b0a ancestor a@1e71731e6fbb
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
  updating: b 1/2 files (50.00%)
   a: versions differ -> m
  updating: a 2/2 files (100.00%)
  picked tool 'true' for a (binary False symlink False)
  merging a
  my a@c19d34741b0a+ other a@1e71731e6fbb ancestor a@c19d34741b0a
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
