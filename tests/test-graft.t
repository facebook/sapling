  $ cat >> $HGRCPATH <<EOF
  > [extdiff]
  > # for portability:
  > pdiff = sh "$RUNTESTDIR/pdiff"
  > EOF

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

  $ hg log -G --template '{author}@{rev}.{phase}: {desc}\n'
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
  
Can't continue without starting:

  $ hg rm -q e
  $ hg graft --continue
  abort: no graft in progress
  [255]
  $ hg revert -r . -q e

Need to specify a rev:

  $ hg graft
  abort: no revisions specified
  [255]

Can't graft ancestor:

  $ hg graft 1 2
  skipping ancestor revision 1:5d205f8b35b6
  skipping ancestor revision 2:5c095ad7e90f
  [255]

Specify revisions with -r:

  $ hg graft -r 1 -r 2
  skipping ancestor revision 1:5d205f8b35b6
  skipping ancestor revision 2:5c095ad7e90f
  [255]

  $ hg graft -r 1 2
  warning: inconsistent use of --rev might give unexpected revision ordering!
  skipping ancestor revision 2:5c095ad7e90f
  skipping ancestor revision 1:5d205f8b35b6
  [255]

Can't graft with dirty wd:

  $ hg up -q 0
  $ echo foo > a
  $ hg graft 1
  abort: uncommitted changes
  [255]
  $ hg revert a

Graft a rename:
(this also tests that editor is invoked if '--edit' is specified)

  $ hg status --rev "2^1" --rev 2
  A b
  R a
  $ HGEDITOR=cat hg graft 2 -u foo --edit
  grafting 2:5c095ad7e90f "2"
  merging a and b to b
  2
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: foo
  HG: branch 'default'
  HG: added b
  HG: removed a
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
(this also tests that editor is not invoked if '--edit' is not specified)

  $ hg graft 1 5 4 3 'merge()' 2 -n
  skipping ungraftable merge revision 6
  skipping revision 2:5c095ad7e90f (already grafted to 7:ef0ef43d49e7)
  grafting 1:5d205f8b35b6 "1"
  grafting 5:97f8bfe72746 "5"
  grafting 4:9c233e8e184d "4"
  grafting 3:4c60f11aa304 "3"

  $ HGEDITOR=cat hg graft 1 5 'merge()' 2 --debug
  skipping ungraftable merge revision 6
  scanning for duplicate grafts
  skipping revision 2:5c095ad7e90f (already grafted to 7:ef0ef43d49e7)
  grafting 1:5d205f8b35b6 "1"
    searching for copies back to rev 1
    unmatched files in local:
     b
    all copies found (* = to merge, ! = divergent, % = renamed and deleted):
     src: 'a' -> dst: 'b' *
    checking for directory renames
  resolving manifests
   branchmerge: True, force: True, partial: False
   ancestor: 68795b066622, local: ef0ef43d49e7+, remote: 5d205f8b35b6
   preserving b for resolve of b
  starting 4 threads for background file closing (?)
   b: local copied/moved from a -> m (premerge)
  picked tool ':merge' for b (binary False symlink False changedelete False)
  merging b and a to b
  my b@ef0ef43d49e7+ other a@5d205f8b35b6 ancestor a@68795b066622
   premerge successful
  committing files:
  b
  committing manifest
  committing changelog
  updating the branch cache
  grafting 5:97f8bfe72746 "5"
    searching for copies back to rev 1
    unmatched files in other (from topological common ancestor):
     c
  resolving manifests
   branchmerge: True, force: True, partial: False
   ancestor: 4c60f11aa304, local: 6b9e5368ca4e+, remote: 97f8bfe72746
   e: remote is newer -> g
  getting e
  committing files:
  e
  committing manifest
  committing changelog
  updating the branch cache
  $ HGEDITOR=cat hg graft 4 3 --log --debug
  scanning for duplicate grafts
  grafting 4:9c233e8e184d "4"
    searching for copies back to rev 1
    unmatched files in other (from topological common ancestor):
     c
  resolving manifests
   branchmerge: True, force: True, partial: False
   ancestor: 4c60f11aa304, local: 1905859650ec+, remote: 9c233e8e184d
   preserving e for resolve of e
   d: remote is newer -> g
  getting d
   e: versions differ -> m (premerge)
  picked tool ':merge' for e (binary False symlink False changedelete False)
  merging e
  my e@1905859650ec+ other e@9c233e8e184d ancestor e@4c60f11aa304
   e: versions differ -> m (merge)
  picked tool ':merge' for e (binary False symlink False changedelete False)
  my e@1905859650ec+ other e@9c233e8e184d ancestor e@4c60f11aa304
  warning: conflicts while merging e! (edit, then use 'hg resolve --mark')
  abort: unresolved conflicts, can't continue
  (use 'hg resolve' and 'hg graft --continue --log')
  [255]

Summary should mention graft:

  $ hg summary |grep graft
  commit: 2 modified, 2 unknown, 1 unresolved (graft in progress)

Using status to get more context

  $ hg status --verbose
  M d
  M e
  ? a.orig
  ? e.orig
  # The repository is in an unfinished *graft* state.
  
  # Unresolved merge conflicts:
  # 
  #     e
  # 
  # To mark files as resolved:  hg resolve --mark FILE
  
  # To continue:                hg graft --continue
  # To abort:                   hg update --clean .    (warning: this will discard uncommitted changes)
  

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

  $ hg strip . --config extensions.strip=
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/a/.hg/strip-backup/*-backup.hg (glob)

Graft again:

  $ hg graft 1 5 4 3 'merge()' 2
  skipping ungraftable merge revision 6
  skipping revision 2:5c095ad7e90f (already grafted to 7:ef0ef43d49e7)
  skipping revision 1:5d205f8b35b6 (already grafted to 8:6b9e5368ca4e)
  skipping revision 5:97f8bfe72746 (already grafted to 9:1905859650ec)
  grafting 4:9c233e8e184d "4"
  merging e
  warning: conflicts while merging e! (edit, then use 'hg resolve --mark')
  abort: unresolved conflicts, can't continue
  (use 'hg resolve' and 'hg graft --continue')
  [255]

Continue without resolve should fail:

  $ hg graft -c
  grafting 4:9c233e8e184d "4"
  abort: unresolved merge conflicts (see 'hg help resolve')
  [255]

Fix up:

  $ echo b > e
  $ hg resolve -m e
  (no more unresolved files)
  continue: hg graft --continue

Continue with a revision should fail:

  $ hg graft -c 6
  abort: can't specify --continue and revisions
  [255]

  $ hg graft -c -r 6
  abort: can't specify --continue and revisions
  [255]

Continue for real, clobber usernames

  $ hg graft -c -U
  grafting 4:9c233e8e184d "4"
  grafting 3:4c60f11aa304 "3"

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

  $ hg log -G --template '{author}@{rev}.{phase}: {desc}\n'
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
  grafting 7:ef0ef43d49e7 "2"

  $ hg log -r 7 --template '{rev}:{node}\n'
  7:ef0ef43d49e79e81ddafdc7997401ba0041efc82
  $ hg log -r 2 --template '{rev}:{node}\n'
  2:5c095ad7e90f871700f02dd1fa5012cb4498a2d4

  $ hg log --debug -r tip
  changeset:   13:7a4785234d87ec1aa420ed6b11afe40fa73e12a9
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
  extra:       intermediate-source=ef0ef43d49e79e81ddafdc7997401ba0041efc82
  extra:       source=5c095ad7e90f871700f02dd1fa5012cb4498a2d4
  description:
  2
  
  
Disallow grafting an already grafted cset onto its original branch
  $ hg up -q 6
  $ hg graft 7
  skipping already grafted revision 7:ef0ef43d49e7 (was grafted from 2:5c095ad7e90f)
  [255]

  $ hg pdiff --config extensions.extdiff= --patch -r 2 -r 13
  --- */hg-5c095ad7e90f.patch	* (glob)
  +++ */hg-7a4785234d87.patch	* (glob)
  @@ -1,18 +1,18 @@
   # HG changeset patch
  -# User test
  +# User foo
   # Date 0 0
   #      Thu Jan 01 00:00:00 1970 +0000
  -# Node ID 5c095ad7e90f871700f02dd1fa5012cb4498a2d4
  -# Parent  5d205f8b35b66bc36375c9534ffd3237730e8f04
  +# Node ID 7a4785234d87ec1aa420ed6b11afe40fa73e12a9
  +# Parent  b592ea63bb0c19a6c5c44685ee29a2284f9f1b8f
   2
   
  -diff -r 5d205f8b35b6 -r 5c095ad7e90f a
  +diff -r b592ea63bb0c -r 7a4785234d87 a
   --- a/a	Thu Jan 01 00:00:00 1970 +0000
   +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
   @@ -1,1 +0,0 @@
  --b
  -diff -r 5d205f8b35b6 -r 5c095ad7e90f b
  +-a
  +diff -r b592ea63bb0c -r 7a4785234d87 b
   --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
   +++ b/b	Thu Jan 01 00:00:00 1970 +0000
   @@ -0,0 +1,1 @@
  -+b
  ++a
  [1]

  $ hg pdiff --config extensions.extdiff= --patch -r 2 -r 13 -X .
  --- */hg-5c095ad7e90f.patch	* (glob)
  +++ */hg-7a4785234d87.patch	* (glob)
  @@ -1,8 +1,8 @@
   # HG changeset patch
  -# User test
  +# User foo
   # Date 0 0
   #      Thu Jan 01 00:00:00 1970 +0000
  -# Node ID 5c095ad7e90f871700f02dd1fa5012cb4498a2d4
  -# Parent  5d205f8b35b66bc36375c9534ffd3237730e8f04
  +# Node ID 7a4785234d87ec1aa420ed6b11afe40fa73e12a9
  +# Parent  b592ea63bb0c19a6c5c44685ee29a2284f9f1b8f
   2
   
  [1]

Disallow grafting already grafted csets with the same origin onto each other
  $ hg up -q 13
  $ hg graft 2
  skipping revision 2:5c095ad7e90f (already grafted to 13:7a4785234d87)
  [255]
  $ hg graft 7
  skipping already grafted revision 7:ef0ef43d49e7 (13:7a4785234d87 also has origin 2:5c095ad7e90f)
  [255]

  $ hg up -q 7
  $ hg graft 2
  skipping revision 2:5c095ad7e90f (already grafted to 7:ef0ef43d49e7)
  [255]
  $ hg graft tip
  skipping already grafted revision 13:7a4785234d87 (7:ef0ef43d49e7 also has origin 2:5c095ad7e90f)
  [255]

Graft with --log

  $ hg up -Cq 1
  $ hg graft 3 --log -u foo
  grafting 3:4c60f11aa304 "3"
  warning: can't find ancestor for 'c' copied from 'b'!
  $ hg log --template '{rev}:{node|short} {parents} {desc}\n' -r tip
  14:0c921c65ef1e 1:5d205f8b35b6  3
  (grafted from 4c60f11aa304a54ae1c199feb94e7fc771e51ed8)

Resolve conflicted graft
  $ hg up -q 0
  $ echo b > a
  $ hg ci -m 8
  created new head
  $ echo c > a
  $ hg ci -m 9
  $ hg graft 1 --tool internal:fail
  grafting 1:5d205f8b35b6 "1"
  abort: unresolved conflicts, can't continue
  (use 'hg resolve' and 'hg graft --continue')
  [255]
  $ hg resolve --all
  merging a
  warning: conflicts while merging a! (edit, then use 'hg resolve --mark')
  [1]
  $ cat a
  <<<<<<< local: aaa4406d4f0a - test: 9
  c
  =======
  b
  >>>>>>> graft: 5d205f8b35b6 - bar: 1
  $ echo b > a
  $ hg resolve -m a
  (no more unresolved files)
  continue: hg graft --continue
  $ hg graft -c
  grafting 1:5d205f8b35b6 "1"
  $ hg export tip --git
  # HG changeset patch
  # User bar
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID f67661df0c4804d301f064f332b57e7d5ddaf2be
  # Parent  aaa4406d4f0ae9befd6e58c82ec63706460cbca6
  1
  
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,1 @@
  -c
  +b

Resolve conflicted graft with rename
  $ echo c > a
  $ hg ci -m 10
  $ hg graft 2 --tool internal:fail
  grafting 2:5c095ad7e90f "2"
  abort: unresolved conflicts, can't continue
  (use 'hg resolve' and 'hg graft --continue')
  [255]
  $ hg resolve --all
  merging a and b to b
  (no more unresolved files)
  continue: hg graft --continue
  $ hg graft -c
  grafting 2:5c095ad7e90f "2"
  $ hg export tip --git
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID 9627f653b421c61fc1ea4c4e366745070fa3d2bc
  # Parent  ee295f490a40b97f3d18dd4c4f1c8936c233b612
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
  
Test that the graft and transplant markers in extra are converted, allowing
origin() to still work.  Note that these recheck the immediately preceeding two
tests.
  $ hg --quiet --config extensions.convert= --config convert.hg.saverev=True convert . ../converted

The graft case
  $ hg -R ../converted log -r 7 --template "{rev}: {node}\n{join(extras, '\n')}\n"
  7: 7ae846e9111fc8f57745634250c7b9ac0a60689b
  branch=default
  convert_revision=ef0ef43d49e79e81ddafdc7997401ba0041efc82
  source=e0213322b2c1a5d5d236c74e79666441bee67a7d
  $ hg -R ../converted log -r 'origin(7)'
  changeset:   2:e0213322b2c1
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2
  
Test that template correctly expands more than one 'extra' (issue4362), and that
'intermediate-source' is converted.
  $ hg -R ../converted log -r 13 --template "{extras % ' Extra: {extra}\n'}"
   Extra: branch=default
   Extra: convert_revision=7a4785234d87ec1aa420ed6b11afe40fa73e12a9
   Extra: intermediate-source=7ae846e9111fc8f57745634250c7b9ac0a60689b
   Extra: source=e0213322b2c1a5d5d236c74e79666441bee67a7d

The transplant case
  $ hg -R ../converted log -r tip --template "{rev}: {node}\n{join(extras, '\n')}\n"
  21: fbb6c5cc81002f2b4b49c9d731404688bcae5ade
  branch=dev
  convert_revision=7e61b508e709a11d28194a5359bc3532d910af21
  transplant_source=z\xe8F\xe9\x11\x1f\xc8\xf5wEcBP\xc7\xb9\xac\n`h\x9b
  $ hg -R ../converted log -r 'origin(tip)'
  changeset:   2:e0213322b2c1
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
  
  changeset:   13:7a4785234d87
  user:        foo
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2
  
  changeset:   14:0c921c65ef1e
  parent:      1:5d205f8b35b6
  user:        foo
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     3
  
  changeset:   17:f67661df0c48
  user:        bar
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1
  
  changeset:   19:9627f653b421
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
  
  changeset:   13:7a4785234d87
  user:        foo
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2
  
  changeset:   19:9627f653b421
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
  warning: can't find ancestor for 'b' copied from 'a'!
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
  
  changeset:   13:7a4785234d87
  user:        foo
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2
  
  changeset:   19:9627f653b421
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2
  
  changeset:   21:7e61b508e709
  branch:      dev
  user:        foo
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2
  
  changeset:   22:d1cb6591fa4b
  branch:      dev
  tag:         tip
  user:        foo
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2
  

graft works on complex revset

  $ hg graft 'origin(13) or destination(origin(13))'
  skipping ancestor revision 21:7e61b508e709
  skipping ancestor revision 22:d1cb6591fa4b
  skipping revision 2:5c095ad7e90f (already grafted to 22:d1cb6591fa4b)
  grafting 7:ef0ef43d49e7 "2"
  warning: can't find ancestor for 'b' copied from 'a'!
  grafting 13:7a4785234d87 "2"
  warning: can't find ancestor for 'b' copied from 'a'!
  grafting 19:9627f653b421 "2"
  merging b
  warning: can't find ancestor for 'b' copied from 'a'!

graft with --force (still doesn't graft merges)

  $ hg graft 19 0 6
  skipping ungraftable merge revision 6
  skipping ancestor revision 0:68795b066622
  skipping already grafted revision 19:9627f653b421 (22:d1cb6591fa4b also has origin 2:5c095ad7e90f)
  [255]
  $ hg graft 19 0 6 --force
  skipping ungraftable merge revision 6
  grafting 19:9627f653b421 "2"
  merging b
  warning: can't find ancestor for 'b' copied from 'a'!
  grafting 0:68795b066622 "0"

graft --force after backout

  $ echo abc > a
  $ hg ci -m 28
  $ hg backout 28
  reverting a
  changeset 29:53177ba928f6 backs out changeset 28:50a516bb8b57
  $ hg graft 28
  skipping ancestor revision 28:50a516bb8b57
  [255]
  $ hg graft 28 --force
  grafting 28:50a516bb8b57 "28"
  merging a
  $ cat a
  abc

graft --continue after --force

  $ echo def > a
  $ hg ci -m 31
  $ hg graft 28 --force --tool internal:fail
  grafting 28:50a516bb8b57 "28"
  abort: unresolved conflicts, can't continue
  (use 'hg resolve' and 'hg graft --continue')
  [255]
  $ hg resolve --all
  merging a
  warning: conflicts while merging a! (edit, then use 'hg resolve --mark')
  [1]
  $ echo abc > a
  $ hg resolve -m a
  (no more unresolved files)
  continue: hg graft --continue
  $ hg graft -c
  grafting 28:50a516bb8b57 "28"
  $ cat a
  abc

Continue testing same origin policy, using revision numbers from test above
but do some destructive editing of the repo:

  $ hg up -qC 7
  $ hg tag -l -r 13 tmp
  $ hg --config extensions.strip= strip 2
  saved backup bundle to $TESTTMP/a/.hg/strip-backup/5c095ad7e90f-d323a1e4-backup.hg (glob)
  $ hg graft tmp
  skipping already grafted revision 8:7a4785234d87 (2:ef0ef43d49e7 also has unknown origin 5c095ad7e90f)
  [255]

Empty graft

  $ hg up -qr 26
  $ hg tag -f something
  $ hg graft -qr 27
  $ hg graft -f 27
  grafting 27:ed6c7e54e319 "28"
  note: graft of 27:ed6c7e54e319 created no changes to commit

  $ cd ..

Graft to duplicate a commit

  $ hg init graftsibling
  $ cd graftsibling
  $ touch a
  $ hg commit -qAm a
  $ touch b
  $ hg commit -qAm b
  $ hg log -G -T '{rev}\n'
  @  1
  |
  o  0
  
  $ hg up -q 0
  $ hg graft -r 1
  grafting 1:0e067c57feba "b" (tip)
  $ hg log -G -T '{rev}\n'
  @  2
  |
  | o  1
  |/
  o  0
  
Graft to duplicate a commit twice

  $ hg up -q 0
  $ hg graft -r 2
  grafting 2:044ec77f6389 "b" (tip)
  $ hg log -G -T '{rev}\n'
  @  3
  |
  | o  2
  |/
  | o  1
  |/
  o  0
  
Graft from behind a move or rename
==================================

NOTE: This is affected by issue5343, and will need updating when it's fixed

Possible cases during a regular graft (when ca is between cta and c2):

name | c1<-cta | cta<->ca | ca->c2
A.0  |         |          |
A.1  |    X    |          |
A.2  |         |     X    |
A.3  |         |          |   X
A.4  |    X    |     X    |
A.5  |    X    |          |   X
A.6  |         |     X    |   X
A.7  |    X    |     X    |   X

A.0 is trivial, and doesn't need copy tracking.
For A.1, a forward rename is recorded in the c1 pass, to be followed later.
In A.2, the rename is recorded in the c2 pass and followed backwards.
A.3 is recorded in the c2 pass as a forward rename to be duplicated on target.
In A.4, both passes of checkcopies record incomplete renames, which are
then joined in mergecopies to record a rename to be followed.
In A.5 and A.7, the c1 pass records an incomplete rename, while the c2 pass
records an incomplete divergence. The incomplete rename is then joined to the
appropriate side of the incomplete divergence, and the result is recorded as a
divergence. The code doesn't distinguish at all between these two cases, since
the end result of them is the same: an incomplete divergence joined with an
incomplete rename into a divergence.
Finally, A.6 records a divergence entirely in the c2 pass.

A.4 has a degenerate case a<-b<-a->a, where checkcopies isn't needed at all.
A.5 has a special case a<-b<-b->a, which is treated like a<-b->a in a merge.
A.6 has a special case a<-a<-b->a. Here, checkcopies will find a spurious
incomplete divergence, which is in fact complete. This is handled later in
mergecopies.
A.7 has 4 special cases: a<-b<-a->b (the "ping-pong" case), a<-b<-c->b,
a<-b<-a->c and a<-b<-c->a. Of these, only the "ping-pong" case is interesting,
the others are fairly trivial (a<-b<-c->b and a<-b<-a->c proceed like the base
case, a<-b<-c->a is treated the same as a<-b<-b->a).

f5a therefore tests the "ping-pong" rename case, where a file is renamed to the
same name on both branches, then the rename is backed out on one branch, and
the backout is grafted to the other branch. This creates a challenging rename
sequence of a<-b<-a->b in the graft target, topological CA, graft CA and graft
source, respectively. Since rename detection will run on the c1 side for such a
sequence (as for technical reasons, we split the c1 and c2 sides not at the
graft CA, but rather at the topological CA), it will pick up a false rename,
and cause a spurious merge conflict. This false rename is always exactly the
reverse of the true rename that would be detected on the c2 side, so we can
correct for it by detecting this condition and reversing as necessary.

First, set up the repository with commits to be grafted

  $ hg init ../graftmove
  $ cd ../graftmove
  $ echo c1a > f1a
  $ echo c2a > f2a
  $ echo c3a > f3a
  $ echo c4a > f4a
  $ echo c5a > f5a
  $ hg ci -qAm A0
  $ hg mv f1a f1b
  $ hg mv f3a f3b
  $ hg mv f5a f5b
  $ hg ci -qAm B0
  $ echo c1c > f1b
  $ hg mv f2a f2c
  $ hg mv f5b f5a
  $ echo c5c > f5a
  $ hg ci -qAm C0
  $ hg mv f3b f3d
  $ echo c4d > f4a
  $ hg ci -qAm D0
  $ hg log -G
  @  changeset:   3:b69f5839d2d9
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     D0
  |
  o  changeset:   2:f58c7e2b28fa
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     C0
  |
  o  changeset:   1:3d7bba921b5d
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     B0
  |
  o  changeset:   0:11f7a1b56675
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     A0
  

Test the cases A.2 (f1x), A.3 (f2x) and a special case of A.6 (f5x) where the
two renames actually converge to the same name (thus no actual divergence).

  $ hg up -q 'desc("A0")'
  $ HGEDITOR="echo C1 >" hg graft -r 'desc("C0")' --edit
  grafting 2:f58c7e2b28fa "C0"
  merging f1a and f1b to f1a
  merging f5a
  warning: can't find ancestor for 'f5a' copied from 'f5b'!
  $ hg status --change .
  M f1a
  M f5a
  A f2c
  R f2a
  $ hg cat f1a
  c1c
  $ hg cat f1b
  f1b: no such file in rev c9763722f9bd
  [1]

Test the cases A.0 (f4x) and A.6 (f3x)

  $ HGEDITOR="echo D1 >" hg graft -r 'desc("D0")' --edit
  grafting 3:b69f5839d2d9 "D0"
  note: possible conflict - f3b was renamed multiple times to:
   f3d
   f3a
  warning: can't find ancestor for 'f3d' copied from 'f3b'!

Set up the repository for some further tests

  $ hg up -q "min(desc("A0"))"
  $ hg mv f1a f1e
  $ echo c2e > f2a
  $ hg mv f3a f3e
  $ hg mv f4a f4e
  $ hg mv f5a f5b
  $ hg ci -qAm "E0"
  $ hg log -G
  @  changeset:   6:6bd1736cab86
  |  tag:         tip
  |  parent:      0:11f7a1b56675
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     E0
  |
  | o  changeset:   5:560daee679da
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     D1
  | |
  | o  changeset:   4:c9763722f9bd
  |/   parent:      0:11f7a1b56675
  |    user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     C1
  |
  | o  changeset:   3:b69f5839d2d9
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     D0
  | |
  | o  changeset:   2:f58c7e2b28fa
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     C0
  | |
  | o  changeset:   1:3d7bba921b5d
  |/   user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     B0
  |
  o  changeset:   0:11f7a1b56675
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     A0
  

Test the cases A.4 (f1x), the "ping-pong" special case of A.7 (f5x),
and A.3 with a local content change to be preserved (f2x).

  $ HGEDITOR="echo C2 >" hg graft -r 'desc("C0")' --edit
  grafting 2:f58c7e2b28fa "C0"
  merging f1e and f1b to f1e
  merging f2a and f2c to f2c
  merging f5b and f5a to f5a

Test the cases A.1 (f4x) and A.7 (f3x).

  $ HGEDITOR="echo D2 >" hg graft -r 'desc("D0")' --edit
  grafting 3:b69f5839d2d9 "D0"
  note: possible conflict - f3b was renamed multiple times to:
   f3e
   f3d
  merging f4e and f4a to f4e
  warning: can't find ancestor for 'f3d' copied from 'f3b'!

Check the results of the grafts tested

  $ hg log -CGv --patch --git
  @  changeset:   8:93ee502e8b0a
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  files:       f3d f4e
  |  description:
  |  D2
  |
  |
  |  diff --git a/f3d b/f3d
  |  new file mode 100644
  |  --- /dev/null
  |  +++ b/f3d
  |  @@ -0,0 +1,1 @@
  |  +c3a
  |  diff --git a/f4e b/f4e
  |  --- a/f4e
  |  +++ b/f4e
  |  @@ -1,1 +1,1 @@
  |  -c4a
  |  +c4d
  |
  o  changeset:   7:539cf145f496
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  files:       f1e f2a f2c f5a f5b
  |  copies:      f2c (f2a) f5a (f5b)
  |  description:
  |  C2
  |
  |
  |  diff --git a/f1e b/f1e
  |  --- a/f1e
  |  +++ b/f1e
  |  @@ -1,1 +1,1 @@
  |  -c1a
  |  +c1c
  |  diff --git a/f2a b/f2c
  |  rename from f2a
  |  rename to f2c
  |  diff --git a/f5b b/f5a
  |  rename from f5b
  |  rename to f5a
  |  --- a/f5b
  |  +++ b/f5a
  |  @@ -1,1 +1,1 @@
  |  -c5a
  |  +c5c
  |
  o  changeset:   6:6bd1736cab86
  |  parent:      0:11f7a1b56675
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  files:       f1a f1e f2a f3a f3e f4a f4e f5a f5b
  |  copies:      f1e (f1a) f3e (f3a) f4e (f4a) f5b (f5a)
  |  description:
  |  E0
  |
  |
  |  diff --git a/f1a b/f1e
  |  rename from f1a
  |  rename to f1e
  |  diff --git a/f2a b/f2a
  |  --- a/f2a
  |  +++ b/f2a
  |  @@ -1,1 +1,1 @@
  |  -c2a
  |  +c2e
  |  diff --git a/f3a b/f3e
  |  rename from f3a
  |  rename to f3e
  |  diff --git a/f4a b/f4e
  |  rename from f4a
  |  rename to f4e
  |  diff --git a/f5a b/f5b
  |  rename from f5a
  |  rename to f5b
  |
  | o  changeset:   5:560daee679da
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  files:       f3d f4a
  | |  description:
  | |  D1
  | |
  | |
  | |  diff --git a/f3d b/f3d
  | |  new file mode 100644
  | |  --- /dev/null
  | |  +++ b/f3d
  | |  @@ -0,0 +1,1 @@
  | |  +c3a
  | |  diff --git a/f4a b/f4a
  | |  --- a/f4a
  | |  +++ b/f4a
  | |  @@ -1,1 +1,1 @@
  | |  -c4a
  | |  +c4d
  | |
  | o  changeset:   4:c9763722f9bd
  |/   parent:      0:11f7a1b56675
  |    user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    files:       f1a f2a f2c f5a
  |    copies:      f2c (f2a)
  |    description:
  |    C1
  |
  |
  |    diff --git a/f1a b/f1a
  |    --- a/f1a
  |    +++ b/f1a
  |    @@ -1,1 +1,1 @@
  |    -c1a
  |    +c1c
  |    diff --git a/f2a b/f2c
  |    rename from f2a
  |    rename to f2c
  |    diff --git a/f5a b/f5a
  |    --- a/f5a
  |    +++ b/f5a
  |    @@ -1,1 +1,1 @@
  |    -c5a
  |    +c5c
  |
  | o  changeset:   3:b69f5839d2d9
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  files:       f3b f3d f4a
  | |  copies:      f3d (f3b)
  | |  description:
  | |  D0
  | |
  | |
  | |  diff --git a/f3b b/f3d
  | |  rename from f3b
  | |  rename to f3d
  | |  diff --git a/f4a b/f4a
  | |  --- a/f4a
  | |  +++ b/f4a
  | |  @@ -1,1 +1,1 @@
  | |  -c4a
  | |  +c4d
  | |
  | o  changeset:   2:f58c7e2b28fa
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  files:       f1b f2a f2c f5a f5b
  | |  copies:      f2c (f2a) f5a (f5b)
  | |  description:
  | |  C0
  | |
  | |
  | |  diff --git a/f1b b/f1b
  | |  --- a/f1b
  | |  +++ b/f1b
  | |  @@ -1,1 +1,1 @@
  | |  -c1a
  | |  +c1c
  | |  diff --git a/f2a b/f2c
  | |  rename from f2a
  | |  rename to f2c
  | |  diff --git a/f5b b/f5a
  | |  rename from f5b
  | |  rename to f5a
  | |  --- a/f5b
  | |  +++ b/f5a
  | |  @@ -1,1 +1,1 @@
  | |  -c5a
  | |  +c5c
  | |
  | o  changeset:   1:3d7bba921b5d
  |/   user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    files:       f1a f1b f3a f3b f5a f5b
  |    copies:      f1b (f1a) f3b (f3a) f5b (f5a)
  |    description:
  |    B0
  |
  |
  |    diff --git a/f1a b/f1b
  |    rename from f1a
  |    rename to f1b
  |    diff --git a/f3a b/f3b
  |    rename from f3a
  |    rename to f3b
  |    diff --git a/f5a b/f5b
  |    rename from f5a
  |    rename to f5b
  |
  o  changeset:   0:11f7a1b56675
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     files:       f1a f2a f3a f4a f5a
     description:
     A0
  
  
     diff --git a/f1a b/f1a
     new file mode 100644
     --- /dev/null
     +++ b/f1a
     @@ -0,0 +1,1 @@
     +c1a
     diff --git a/f2a b/f2a
     new file mode 100644
     --- /dev/null
     +++ b/f2a
     @@ -0,0 +1,1 @@
     +c2a
     diff --git a/f3a b/f3a
     new file mode 100644
     --- /dev/null
     +++ b/f3a
     @@ -0,0 +1,1 @@
     +c3a
     diff --git a/f4a b/f4a
     new file mode 100644
     --- /dev/null
     +++ b/f4a
     @@ -0,0 +1,1 @@
     +c4a
     diff --git a/f5a b/f5a
     new file mode 100644
     --- /dev/null
     +++ b/f5a
     @@ -0,0 +1,1 @@
     +c5a
  
  $ hg cat f2c
  c2e

Check superfluous filemerge of files renamed in the past but untouched by graft

  $ echo a > a
  $ hg ci -qAma
  $ hg mv a b
  $ echo b > b
  $ hg ci -qAmb
  $ echo c > c
  $ hg ci -qAmc
  $ hg up -q .~2
  $ hg graft tip -qt:fail

  $ cd ..

Graft a change into a new file previously grafted into a renamed directory

  $ hg init dirmovenewfile
  $ cd dirmovenewfile
  $ mkdir a
  $ echo a > a/a
  $ hg ci -qAma
  $ echo x > a/x
  $ hg ci -qAmx
  $ hg up -q 0
  $ hg mv -q a b
  $ hg ci -qAmb
  $ hg graft -q 1 # a/x grafted as b/x, but no copy information recorded
  $ hg up -q 1
  $ echo y > a/x
  $ hg ci -qAmy
  $ hg up -q 3
  $ hg graft -q 4
  $ hg status --change .
  M b/x

Prepare for test of skipped changesets and how merges can influence it:

  $ hg merge -q -r 1 --tool :local
  $ hg ci -m m
  $ echo xx >> b/x
  $ hg ci -m xx

  $ hg log -G -T '{rev} {desc|firstline}'
  @  7 xx
  |
  o    6 m
  |\
  | o  5 y
  | |
  +---o  4 y
  | |
  | o  3 x
  | |
  | o  2 b
  | |
  o |  1 x
  |/
  o  0 a
  
Grafting of plain changes correctly detects that 3 and 5 should be skipped:

  $ hg up -qCr 4
  $ hg graft --tool :local -r 2::5
  skipping already grafted revision 3:ca093ca2f1d9 (was grafted from 1:13ec5badbf2a)
  skipping already grafted revision 5:43e9eb70dab0 (was grafted from 4:6c9a1289e5f1)
  grafting 2:42127f193bcd "b"

Extending the graft range to include a (skipped) merge of 3 will not prevent us from
also detecting that both 3 and 5 should be skipped:

  $ hg up -qCr 4
  $ hg graft --tool :local -r 2::7
  skipping ungraftable merge revision 6
  skipping already grafted revision 3:ca093ca2f1d9 (was grafted from 1:13ec5badbf2a)
  skipping already grafted revision 5:43e9eb70dab0 (was grafted from 4:6c9a1289e5f1)
  grafting 2:42127f193bcd "b"
  grafting 7:d3c3f2b38ecc "xx"
  note: graft of 7:d3c3f2b38ecc created no changes to commit

  $ cd ..
