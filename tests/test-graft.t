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

  $ HGEDITOR=cat hg graft 1 5 4 3 'merge()' 2 --debug
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
   b: local copied/moved from a -> m
  picked tool 'internal:merge' for b (binary False symlink False)
  merging b and a to b
  my b@ef0ef43d49e7+ other a@5d205f8b35b6 ancestor a@68795b066622
   premerge successful
  committing files:
  b
  committing manifest
  committing changelog
  grafting 5:97f8bfe72746 "5"
    searching for copies back to rev 1
  resolving manifests
   branchmerge: True, force: True, partial: False
   ancestor: 4c60f11aa304, local: 6b9e5368ca4e+, remote: 97f8bfe72746
   e: remote is newer -> g
  getting e
   b: remote unchanged -> k
  committing files:
  e
  committing manifest
  committing changelog
  grafting 4:9c233e8e184d "4"
    searching for copies back to rev 1
  resolving manifests
   branchmerge: True, force: True, partial: False
   ancestor: 4c60f11aa304, local: 1905859650ec+, remote: 9c233e8e184d
   preserving e for resolve of e
   d: remote is newer -> g
  getting d
   b: remote unchanged -> k
   e: versions differ -> m
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
  warning: conflicts during merge.
  merging e incomplete! (edit conflicts, then use 'hg resolve --mark')
  abort: unresolved conflicts, can't continue
  (use hg resolve and hg graft --continue)
  [255]

Continue without resolve should fail:

  $ hg graft -c
  grafting 4:9c233e8e184d "4"
  abort: unresolved merge conflicts (see "hg help resolve")
  [255]

Fix up:

  $ echo b > e
  $ hg resolve -m e
  (no more unresolved files)

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
  $ hg log --template '{rev} {parents} {desc}\n' -r tip
  14 1:5d205f8b35b6  3
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
  (use hg resolve and hg graft --continue)
  [255]
  $ hg resolve --all
  merging a
  warning: conflicts during merge.
  merging a incomplete! (edit conflicts, then use 'hg resolve --mark')
  [1]
  $ cat a
  <<<<<<< local: aaa4406d4f0a - test: 9
  c
  =======
  b
  >>>>>>> other: 5d205f8b35b6  - bar: 1
  $ echo b > a
  $ hg resolve -m a
  (no more unresolved files)
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
  (use hg resolve and hg graft --continue)
  [255]
  $ hg resolve --all
  merging a and b to b
  (no more unresolved files)
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
  
Test that template correctly expands more than one 'extra' (issue4362)
  $ hg -R ../converted log -r 7 --template "{extras % ' Extra: {extra}\n'}"
   Extra: branch=default
   Extra: convert_revision=ef0ef43d49e79e81ddafdc7997401ba0041efc82
   Extra: source=e0213322b2c1a5d5d236c74e79666441bee67a7d

The transplant case
  $ hg -R ../converted log -r tip --template "{rev}: {node}\n{join(extras, '\n')}\n"
  21: fbb6c5cc81002f2b4b49c9d731404688bcae5ade
  branch=dev
  convert_revision=7e61b508e709a11d28194a5359bc3532d910af21
  transplant_source=z\xe8F\xe9\x11\x1f\xc8\xf5wEcBP\xc7\xb9\xac (esc)
  `h\x9b (esc)
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
  
  changeset:   14:f64defefacee
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
  (use hg resolve and hg graft --continue)
  [255]
  $ hg resolve --all
  merging a
  warning: conflicts during merge.
  merging a incomplete! (edit conflicts, then use 'hg resolve --mark')
  [1]
  $ echo abc > a
  $ hg resolve -m a
  (no more unresolved files)
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
  
