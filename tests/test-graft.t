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
   overwrite: False, partial: False
   ancestor: 68795b066622, local: d2e44c99fd3f+, remote: 5d205f8b35b6
   b: local copied/moved to a -> m
  preserving b for resolve of b
  updating: b 1/1 files (100.00%)
  b
   b: searching for copy revision for a
   b: copy a:b789fdd96dc2f3bd229c1dd8eedf0fc60e2b68e3
  grafting revision 5
    searching for copies back to rev 1
    unmatched files in local:
     a.orig
  resolving manifests
   overwrite: False, partial: False
   ancestor: 4c60f11aa304, local: 6f5ea6ac8b70+, remote: 97f8bfe72746
   e: remote is newer -> g
  updating: e 1/1 files (100.00%)
  getting e
  e
  grafting revision 4
    searching for copies back to rev 1
    unmatched files in local:
     a.orig
  resolving manifests
   overwrite: False, partial: False
   ancestor: 4c60f11aa304, local: 77eb504366ab+, remote: 9c233e8e184d
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
  merging e incomplete! (edit conflicts, then use 'hg resolve --mark')
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

  $ hg --config extensions.graphlog= log -G --template '{author}@{rev}: {desc}\n'
  @  test@11: 3
  |
  o  test@10: 4
  |
  o  test@9: 5
  |
  o  bar@8: 1
  |
  o  foo@7: 2
  |
  | o    test@6: 6
  | |\
  | | o  test@5: 5
  | | |
  | o |  test@4: 4
  | |/
  | o  baz@3: 3
  | |
  | o  test@2: 2
  | |
  | o  bar@1: 1
  |/
  o  test@0: 0
  
Graft again onto another branch should preserve the original source
  $ hg up -q 0
  $ echo 'g'>g
  $ hg add g
  $ hg ci -m 7
  created new head
  $ hg graft 7
  grafting revision 7

  $ hg log -r 7 --template '{rev}:{node}\n'
  7:d2e44c99fd3f31c176ea4efb9eca9f6306c81756
  $ hg log -r 2 --template '{rev}:{node}\n'
  2:5c095ad7e90f871700f02dd1fa5012cb4498a2d4

  $ hg log --debug -r tip
  changeset:   13:39bb1d13572759bd1e6fc874fed1b12ece047a18
  tag:         tip
  parent:      12:b592ea63bb0c19a6c5c44685ee29a2284f9f1b8f
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    13:0780e055d8f4cd12eadd5a2719481648f336f7a9
  user:        foo
  date:        Thu Jan 01 00:00:00 1970 +0000
  files+:      b
  files-:      a
  extra:       branch=default
  extra:       source=5c095ad7e90f871700f02dd1fa5012cb4498a2d4
  description:
  2
  
  
Disallow grafting an already grafted cset onto its original branch
  $ hg up -q 6
  $ hg graft 7
  skipping already grafted revision 7 (was grafted from 2)
  [255]

Disallow grafting already grafted csets with the same origin onto each other
  $ hg up -q 13
  $ hg graft 2
  skipping already grafted revision 2
  [255]
  $ hg graft 7
  skipping already grafted revision 7 (same origin 2)
  [255]

  $ hg up -q 7
  $ hg graft 2
  skipping already grafted revision 2
  [255]
  $ hg graft tip
  skipping already grafted revision 13 (same origin 2)
  [255]
