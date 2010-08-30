  $ HGMERGE=true; export HGMERGE

  $ mkdir r1
  $ cd r1
  $ hg init
  $ echo a > a
  $ hg addremove
  adding a
  $ hg commit -m "1" -d "1000000 0"

  $ hg clone . ../r2
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ../r2
  $ hg up
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo abc > a
  $ hg diff --nodates
  diff -r 33aaa84a386b a
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
  $ hg commit -m "2" -d "1000000 0"

  $ cd ../r2
  $ hg -q pull ../r1
  $ hg status
  M a
  $ hg parents
  changeset:   0:33aaa84a386b
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     1
  
  $ hg --debug up
    searching for copies back to rev 1
    unmatched files in other:
     b
  resolving manifests
   overwrite False partial False
   ancestor 33aaa84a386b local 33aaa84a386b+ remote 802f095af299
   a: versions differ -> m
   b: remote created -> g
  preserving a for resolve of a
  updating: a 1/2 files (50.00%)
  picked tool 'true' for a (binary False symlink False)
  merging a
  my a@33aaa84a386b+ other a@802f095af299 ancestor a@33aaa84a386b
  updating: b 2/2 files (100.00%)
  getting b
  1 files updated, 1 files merged, 0 files removed, 0 files unresolved
  $ hg parents
  changeset:   1:802f095af299
  tag:         tip
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     2
  
  $ hg --debug up 0
  resolving manifests
   overwrite False partial False
   ancestor 802f095af299 local 802f095af299+ remote 33aaa84a386b
   a: versions differ -> m
   b: other deleted -> r
  preserving a for resolve of a
  updating: b 1/2 files (50.00%)
  removing b
  updating: a 2/2 files (100.00%)
  picked tool 'true' for a (binary False symlink False)
  merging a
  my a@802f095af299+ other a@33aaa84a386b ancestor a@802f095af299
  0 files updated, 1 files merged, 1 files removed, 0 files unresolved
  $ hg parents
  changeset:   0:33aaa84a386b
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     1
  
  $ hg --debug merge || echo failed
  abort: there is nothing to merge - use "hg update" instead
  failed
  $ hg parents
  changeset:   0:33aaa84a386b
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     1
  
  $ hg --debug up
    searching for copies back to rev 1
    unmatched files in other:
     b
  resolving manifests
   overwrite False partial False
   ancestor 33aaa84a386b local 33aaa84a386b+ remote 802f095af299
   a: versions differ -> m
   b: remote created -> g
  preserving a for resolve of a
  updating: a 1/2 files (50.00%)
  picked tool 'true' for a (binary False symlink False)
  merging a
  my a@33aaa84a386b+ other a@802f095af299 ancestor a@33aaa84a386b
  updating: b 2/2 files (100.00%)
  getting b
  1 files updated, 1 files merged, 0 files removed, 0 files unresolved
  $ hg parents
  changeset:   1:802f095af299
  tag:         tip
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     2
  
  $ hg -v history
  changeset:   1:802f095af299
  tag:         tip
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  files:       a b
  description:
  2
  
  
  changeset:   0:33aaa84a386b
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  files:       a
  description:
  1
  
  
  $ hg diff --nodates
  diff -r 802f095af299 a
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
  $ hg commit -m "3" -d "1000000 0"
  created new head

  $ cd ../r2
  $ hg -q pull ../r1
  $ hg status
  M a
  $ hg parents
  changeset:   1:802f095af299
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     2
  
  $ hg --debug up || echo failed
  abort: crosses branches (use 'hg merge' to merge or use 'hg update -C' to discard changes)
  failed
  $ hg --debug merge || echo failed
  abort: outstanding uncommitted changes (use 'hg status' to list changes)
  failed
  $ hg --debug merge -f
    searching for copies back to rev 1
  resolving manifests
   overwrite False partial False
   ancestor 33aaa84a386b local 802f095af299+ remote 030602aee63d
   a: versions differ -> m
   b: versions differ -> m
  preserving a for resolve of a
  preserving b for resolve of b
  updating: a 1/2 files (50.00%)
  picked tool 'true' for a (binary False symlink False)
  merging a
  my a@802f095af299+ other a@030602aee63d ancestor a@33aaa84a386b
  updating: b 2/2 files (100.00%)
  picked tool 'true' for b (binary False symlink False)
  merging b
  my b@802f095af299+ other b@030602aee63d ancestor b@000000000000
  0 files updated, 2 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg parents
  changeset:   1:802f095af299
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     2
  
  changeset:   2:030602aee63d
  tag:         tip
  parent:      0:33aaa84a386b
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     3
  
  $ hg diff --nodates
  diff -r 802f095af299 a
  --- a/a
  +++ b/a
  @@ -1,1 +1,1 @@
  -a2
  +abc


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
