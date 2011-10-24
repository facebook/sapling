Create a repo with some stuff in it:

  $ hg init a
  $ cd a
  $ echo a > a
  $ echo a > d
  $ echo a > e
  $ hg ci -qAm0
  $ echo b > a
  $ hg ci -m1 -u bar
  $ hg mv a b
  $ hg ci -m2
  $ hg cp b c
  $ hg ci -m3 -u baz
  $ echo b > d
  $ echo f > e
  $ hg ci -m4
  $ hg up -q 3
  $ echo b > e
  $ hg branch -q stable
  $ hg ci -m5
  $ hg merge -q default --tool internal:local
  $ hg branch -q default
  $ hg ci -m6

Need to specify a rev:

  $ hg graft
  abort: no revisions specified
  [255]

Can't graft ancestor:

  $ hg graft 1 2
  skipping ancestor revision 1
  skipping ancestor revision 2
  [255]

Can't graft with dirty wd:

  $ hg up -q 0
  $ echo foo > a
  $ hg graft 1
  abort: outstanding uncommitted changes
  [255]
  $ hg revert a

Graft a rename:

  $ hg graft 2 -u foo
  grafting revision 2
  merging a and b to b
  $ hg export tip --git
  # HG changeset patch
  # User foo
  # Date 0 0
  # Node ID d2e44c99fd3f31c176ea4efb9eca9f6306c81756
  # Parent  68795b066622ca79a25816a662041d8f78f3cd9e
  2
  
  diff --git a/a b/b
  rename from a
  rename to b
  --- a/a
  +++ b/b
  @@ -1,1 +1,1 @@
  -a
  +b

Look for extra:source

  $ hg log --debug -r tip
  changeset:   7:d2e44c99fd3f31c176ea4efb9eca9f6306c81756
  tag:         tip
  parent:      0:68795b066622ca79a25816a662041d8f78f3cd9e
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    7:5d59766436fd8fbcd38e7bebef0f6eaf3eebe637
  user:        foo
  date:        Thu Jan 01 00:00:00 1970 +0000
  files+:      b
  files-:      a
  extra:       branch=default
  extra:       source=5c095ad7e90f871700f02dd1fa5012cb4498a2d4
  description:
  2
  
  

Graft out of order, skipping a merge and a duplicate

  $ hg graft 1 5 4 3 'merge()' 2 --debug
  skipping ungraftable merge revision 6
  scanning for duplicate grafts
  skipping already grafted revision 2
  grafting revision 1
    searching for copies back to rev 1
    unmatched files in local:
     a.orig
     b
    all copies found (* = to merge, ! = divergent):
     b -> a *
    checking for directory renames
  resolving manifests
   overwrite False partial False
   ancestor 68795b066622 local d2e44c99fd3f+ remote 5d205f8b35b6
   b: local copied/moved to a -> m
  preserving b for resolve of b
  updating: b 1/1 files (100.00%)
    searching for copies back to rev 1
    unmatched files in local:
     a
    unmatched files in other:
     b
    all copies found (* = to merge, ! = divergent):
     b -> a *
    checking for directory renames
  b
   b: searching for copy revision for a
   b: copy a:b789fdd96dc2f3bd229c1dd8eedf0fc60e2b68e3
  grafting revision 5
    searching for copies back to rev 1
    unmatched files in local:
     a.orig
  resolving manifests
   overwrite False partial False
   ancestor 4c60f11aa304 local 6f5ea6ac8b70+ remote 97f8bfe72746
   e: remote is newer -> g
  updating: e 1/1 files (100.00%)
  getting e
    searching for copies back to rev 1
    unmatched files in local:
     c
    all copies found (* = to merge, ! = divergent):
     c -> b *
    checking for directory renames
  e
  grafting revision 4
    searching for copies back to rev 1
    unmatched files in local:
     a.orig
  resolving manifests
   overwrite False partial False
   ancestor 4c60f11aa304 local 77eb504366ab+ remote 9c233e8e184d
   e: versions differ -> m
   d: remote is newer -> g
  preserving e for resolve of e
  updating: d 1/2 files (50.00%)
  getting d
  updating: e 2/2 files (100.00%)
  picked tool 'internal:merge' for e (binary False symlink False)
  merging e
  my e@77eb504366ab+ other e@9c233e8e184d ancestor e@68795b066622
  warning: conflicts during merge.
  merging e failed!
    searching for copies back to rev 1
    unmatched files in local:
     c
    all copies found (* = to merge, ! = divergent):
     c -> b *
    checking for directory renames
  abort: unresolved conflicts, can't continue
  (use hg resolve and hg graft --continue)
  [255]

Continue without resolve should fail:

  $ hg graft -c
  grafting revision 4
  abort: unresolved merge conflicts (see hg help resolve)
  [255]

Fix up:

  $ echo b > e
  $ hg resolve -m e

Continue with a revision should fail:

  $ hg graft -c 6
  abort: can't specify --continue and revisions
  [255]

Continue for real, clobber usernames

  $ hg graft -c -U
  grafting revision 4
  grafting revision 3

Compare with original:

  $ hg diff -r 6
  $ hg status --rev 0:. -C
  M d
  M e
  A b
    a
  A c
    a
  R a

View graph:

  $ hg --config extensions.graphlog= log -G --template '{author}@rev: {desc}\n'
  @  test@rev: 3
  |
  o  test@rev: 4
  |
  o  test@rev: 5
  |
  o  bar@rev: 1
  |
  o  foo@rev: 2
  |
  | o    test@rev: 6
  | |\
  | | o  test@rev: 5
  | | |
  | o |  test@rev: 4
  | |/
  | o  baz@rev: 3
  | |
  | o  test@rev: 2
  | |
  | o  bar@rev: 1
  |/
  o  test@rev: 0
  
