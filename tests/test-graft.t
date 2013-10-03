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
  $ hg phase --public 3
  $ hg phase --force --secret 6

  $ hg --config extensions.graphlog= log -G --template '{author}@{rev}.{phase}: {desc}\n'
  @    test@6.secret: 6
  |\
  | o  test@5.draft: 5
  | |
  o |  test@4.draft: 4
  |/
  o  baz@3.public: 3
  |
  o  test@2.public: 2
  |
  o  bar@1.public: 1
  |
  o  test@0.public: 0
  

Need to specify a rev:

  $ hg graft
  abort: no revisions specified
  [255]

Can't graft ancestor:

  $ hg graft 1 2
  skipping ancestor revision 1
  skipping ancestor revision 2
  [255]

Specify revisions with -r:

  $ hg graft -r 1 -r 2
  skipping ancestor revision 1
  skipping ancestor revision 2
  [255]

  $ hg graft -r 1 2
  skipping ancestor revision 2
  skipping ancestor revision 1
  [255]

Can't graft with dirty wd:

  $ hg up -q 0
  $ echo foo > a
  $ hg graft 1
  abort: uncommitted changes
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
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID ef0ef43d49e79e81ddafdc7997401ba0041efc82
  # Parent  68795b066622ca79a25816a662041d8f78f3cd9e
  2
  
  diff --git a/a b/b
  rename from a
  rename to b

Look for extra:source

  $ hg log --debug -r tip
  changeset:   7:ef0ef43d49e79e81ddafdc7997401ba0041efc82
  tag:         tip
  phase:       draft
  parent:      0:68795b066622ca79a25816a662041d8f78f3cd9e
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    7:e59b6b228f9cbf9903d5e9abf996e083a1f533eb
  user:        foo
  date:        Thu Jan 01 00:00:00 1970 +0000
  files+:      b
  files-:      a
  extra:       branch=default
  extra:       source=5c095ad7e90f871700f02dd1fa5012cb4498a2d4
  description:
  2
  
  

Graft out of order, skipping a merge and a duplicate

  $ hg graft 1 5 4 3 'merge()' 2 -n
  skipping ungraftable merge revision 6
  skipping revision 2 (already grafted to 7)
  grafting revision 1
  grafting revision 5
  grafting revision 4
  grafting revision 3

  $ hg graft 1 5 4 3 'merge()' 2 --debug
  skipping ungraftable merge revision 6
  scanning for duplicate grafts
  skipping revision 2 (already grafted to 7)
  grafting revision 1
    searching for copies back to rev 1
    unmatched files in local:
     b
    all copies found (* = to merge, ! = divergent, % = renamed and deleted):
     src: 'a' -> dst: 'b' *
    checking for directory renames
  resolving manifests
   branchmerge: True, force: True, partial: False
   ancestor: 68795b066622, local: ef0ef43d49e7+, remote: 5d205f8b35b6
   b: local copied/moved to a -> m
    preserving b for resolve of b
  updating: b 1/1 files (100.00%)
  picked tool 'internal:merge' for b (binary False symlink False)
  merging b and a to b
  my b@ef0ef43d49e7+ other a@5d205f8b35b6 ancestor a@68795b066622
   premerge successful
  b
  grafting revision 5
    searching for copies back to rev 1
  resolving manifests
   branchmerge: True, force: True, partial: False
   ancestor: 4c60f11aa304, local: 6b9e5368ca4e+, remote: 97f8bfe72746
   e: remote is newer -> g
  getting e
  updating: e 1/1 files (100.00%)
  e
  grafting revision 4
    searching for copies back to rev 1
  resolving manifests
   branchmerge: True, force: True, partial: False
   ancestor: 4c60f11aa304, local: 1905859650ec+, remote: 9c233e8e184d
   d: remote is newer -> g
   e: versions differ -> m
    preserving e for resolve of e
  getting d
  updating: d 1/2 files (50.00%)
  updating: e 2/2 files (100.00%)
  picked tool 'internal:merge' for e (binary False symlink False)
  merging e
  my e@1905859650ec+ other e@9c233e8e184d ancestor e@68795b066622
  warning: conflicts during merge.
  merging e incomplete! (edit conflicts, then use 'hg resolve --mark')
  abort: unresolved conflicts, can't continue
  (use hg resolve and hg graft --continue)
  [255]

Commit while interrupted should fail:

  $ hg ci -m 'commit interrupted graft'
  abort: graft in progress
  (use 'hg graft --continue' or 'hg update' to abort)
  [255]

Abort the graft and try committing:

  $ hg up -C .
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo c >> e
  $ hg ci -mtest

  $ hg strip . --config extensions.mq=
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/a/.hg/strip-backup/*-backup.hg (glob)

Graft again:

  $ hg graft 1 5 4 3 'merge()' 2
  skipping ungraftable merge revision 6
  skipping revision 2 (already grafted to 7)
  skipping revision 1 (already grafted to 8)
  skipping revision 5 (already grafted to 9)
  grafting revision 4
  merging e
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

  $ hg graft -c -r 6
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

  $ hg --config extensions.graphlog= log -G --template '{author}@{rev}.{phase}: {desc}\n'
  @  test@11.draft: 3
  |
  o  test@10.draft: 4
  |
  o  test@9.draft: 5
  |
  o  bar@8.draft: 1
  |
  o  foo@7.draft: 2
  |
  | o    test@6.secret: 6
  | |\
  | | o  test@5.draft: 5
  | | |
  | o |  test@4.draft: 4
  | |/
  | o  baz@3.public: 3
  | |
  | o  test@2.public: 2
  | |
  | o  bar@1.public: 1
  |/
  o  test@0.public: 0
  
Graft again onto another branch should preserve the original source
  $ hg up -q 0
  $ echo 'g'>g
  $ hg add g
  $ hg ci -m 7
  created new head
  $ hg graft 7
  grafting revision 7

  $ hg log -r 7 --template '{rev}:{node}\n'
  7:ef0ef43d49e79e81ddafdc7997401ba0041efc82
  $ hg log -r 2 --template '{rev}:{node}\n'
  2:5c095ad7e90f871700f02dd1fa5012cb4498a2d4

  $ hg log --debug -r tip
  changeset:   13:9db0f28fd3747e92c57d015f53b5593aeec53c2d
  tag:         tip
  phase:       draft
  parent:      12:b592ea63bb0c19a6c5c44685ee29a2284f9f1b8f
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    13:dc313617b8c32457c0d589e0dbbedfe71f3cd637
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
  skipping revision 2 (already grafted to 13)
  [255]
  $ hg graft 7
  skipping already grafted revision 7 (13 also has origin 2)
  [255]

  $ hg up -q 7
  $ hg graft 2
  skipping revision 2 (already grafted to 7)
  [255]
  $ hg graft tip
  skipping already grafted revision 13 (7 also has origin 2)
  [255]

Graft with --log

  $ hg up -Cq 1
  $ hg graft 3 --log -u foo
  grafting revision 3
  warning: can't find ancestor for 'c' copied from 'b'!
  $ hg log --template '{rev} {parents} {desc}\n' -r tip
  14 1:5d205f8b35b6  3
  (grafted from 4c60f11aa304a54ae1c199feb94e7fc771e51ed8)

Resolve conflicted graft
  $ hg up -q 0
  $ echo b > a
  $ hg ci -m 8
  created new head
  $ echo a > a
  $ hg ci -m 9
  $ hg graft 1 --tool internal:fail
  grafting revision 1
  abort: unresolved conflicts, can't continue
  (use hg resolve and hg graft --continue)
  [255]
  $ hg resolve --all
  merging a
  $ hg graft -c
  grafting revision 1
  $ hg export tip --git
  # HG changeset patch
  # User bar
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID 64ecd9071ce83c6e62f538d8ce7709d53f32ebf7
  # Parent  4bdb9a9d0b84ffee1d30f0dfc7744cade17aa19c
  1
  
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,1 @@
  -a
  +b

Resolve conflicted graft with rename
  $ echo c > a
  $ hg ci -m 10
  $ hg graft 2 --tool internal:fail
  grafting revision 2
  abort: unresolved conflicts, can't continue
  (use hg resolve and hg graft --continue)
  [255]
  $ hg resolve --all
  merging a and b to b
  $ hg graft -c
  grafting revision 2
  $ hg export tip --git
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID 2e80e1351d6ed50302fe1e05f8bd1d4d412b6e11
  # Parent  e5a51ae854a8bbaaf25cc5c6a57ff46042dadbb4
  2
  
  diff --git a/a b/b
  rename from a
  rename to b

Test simple origin(), with and without args
  $ hg log -r 'origin()'
  changeset:   1:5d205f8b35b6
  user:        bar
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1
  
  changeset:   2:5c095ad7e90f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2
  
  changeset:   3:4c60f11aa304
  user:        baz
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     3
  
  changeset:   4:9c233e8e184d
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     4
  
  changeset:   5:97f8bfe72746
  branch:      stable
  parent:      3:4c60f11aa304
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     5
  
  $ hg log -r 'origin(7)'
  changeset:   2:5c095ad7e90f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2
  
Now transplant a graft to test following through copies
  $ hg up -q 0
  $ hg branch -q dev
  $ hg ci -qm "dev branch"
  $ hg --config extensions.transplant= transplant -q 7
  $ hg log -r 'origin(.)'
  changeset:   2:5c095ad7e90f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2
  
Test simple destination
  $ hg log -r 'destination()'
  changeset:   7:ef0ef43d49e7
  parent:      0:68795b066622
  user:        foo
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2
  
  changeset:   8:6b9e5368ca4e
  user:        bar
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1
  
  changeset:   9:1905859650ec
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     5
  
  changeset:   10:52dc0b4c6907
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     4
  
  changeset:   11:882b35362a6b
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     3
  
  changeset:   13:9db0f28fd374
  user:        foo
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2
  
  changeset:   14:f64defefacee
  parent:      1:5d205f8b35b6
  user:        foo
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     3
  
  changeset:   17:64ecd9071ce8
  user:        bar
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1
  
  changeset:   19:2e80e1351d6e
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2
  
  changeset:   21:7e61b508e709
  branch:      dev
  tag:         tip
  user:        foo
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2
  
  $ hg log -r 'destination(2)'
  changeset:   7:ef0ef43d49e7
  parent:      0:68795b066622
  user:        foo
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2
  
  changeset:   13:9db0f28fd374
  user:        foo
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2
  
  changeset:   19:2e80e1351d6e
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2
  
  changeset:   21:7e61b508e709
  branch:      dev
  tag:         tip
  user:        foo
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2
  
Transplants of grafts can find a destination...
  $ hg log -r 'destination(7)'
  changeset:   21:7e61b508e709
  branch:      dev
  tag:         tip
  user:        foo
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2
  
... grafts of grafts unfortunately can't
  $ hg graft -q 13
  $ hg log -r 'destination(13)'
All copies of a cset
  $ hg log -r 'origin(13) or destination(origin(13))'
  changeset:   2:5c095ad7e90f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2
  
  changeset:   7:ef0ef43d49e7
  parent:      0:68795b066622
  user:        foo
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2
  
  changeset:   13:9db0f28fd374
  user:        foo
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2
  
  changeset:   19:2e80e1351d6e
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2
  
  changeset:   21:7e61b508e709
  branch:      dev
  user:        foo
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2
  
  changeset:   22:1313d0a825e2
  branch:      dev
  tag:         tip
  user:        foo
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2
  
