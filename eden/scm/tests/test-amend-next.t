#chg-compatible

Set up test environment.
  $ configure mutation-norecord
  $ enable amend rebase
  $ showgraph() {
  >   hg log --graph -T "{rev} {bookmarks} {desc|firstline}" | sed \$d
  > }
  $ newrepo

Check help text for new options and removal of unsupported options.
  $ hg next --help
  hg next [OPTIONS]... [STEPS]
  
  aliases: n
  
  check out a child commit
  
  Options:
  
      --newest               always pick the newest child when a changeset has
                             multiple children
      --rebase               rebase each changeset if necessary
      --top                  update to the head of the current stack
      --bookmark             update to the first changeset with a bookmark
      --no-activate-bookmark do not activate the bookmark on the destination
                             changeset
      --towards VALUE        move linearly towards the specified head
   -C --clean                discard uncommitted changes (no backup)
   -B --move-bookmark        move active bookmark
   -m --merge                merge uncommitted changes
  
  (some details hidden, use --verbose to show complete help)

Create stack of commits and go to the bottom.
  $ hg debugbuilddag --mergeable-file +6
  $ hg up 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg book bottom
  $ showgraph
  o  5  r5
  |
  o  4  r4
  |
  o  3  r3
  |
  o  2  r2
  |
  o  1  r1
  |
  @  0 bottom r0

Test invalid argument combinations.
  $ hg next --top 1
  abort: cannot use both number and --top
  [255]
  $ hg next --bookmark 1
  abort: cannot use both number and --bookmark
  [255]
  $ hg next --top --bookmark
  abort: cannot use both --top and --bookmark
  [255]
  $ hg next --top --towards top
  abort: cannot use both --top and --towards
  [255]

Test basic usage.
  $ hg next
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark bottom)
  [*] r1 (glob)

With positional argument.
  $ hg next 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [*] r3 (glob)

Overshoot top of repo.
  $ hg next 5
  reached head changeset
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [*] r5 (glob)

Test --top flag.
  $ hg up bottom
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark bottom)
  $ hg next --top
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark bottom)
  [*] r5 (glob)

Test bookmark navigation.
  $ hg book -r 5 top
  $ hg book -r 3 bookmark
  $ showgraph
  @  5 top r5
  |
  o  4  r4
  |
  o  3 bookmark r3
  |
  o  2  r2
  |
  o  1  r1
  |
  o  0 bottom r0
  $ hg up bottom
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark bottom)
  $ hg next --bookmark
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark bottom)
  [*] (bookmark) r3 (glob)
  (activating bookmark bookmark)
  $ hg next --bookmark
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark bookmark)
  [*] (top) r5 (glob)
  (activating bookmark top)

Test bookmark activation.
  $ hg up bottom
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark bottom)
  $ hg next 3
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark bottom)
  [*] (bookmark) r3 (glob)
  (activating bookmark bookmark)
  $ hg next 2 --no-activate-bookmark
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark bookmark)
  [*] (top) r5 (glob)

Test dirty working copy and --clean.
  $ hg up bottom
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark bottom)
  $ touch test
  $ hg add test
  $ hg st
  A test
  $ hg next
  abort: uncommitted changes
  (use --clean to discard uncommitted changes or --merge to bring them along)
  [255]
  $ hg next --clean
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark bottom)
  [*] r1 (glob)
  $ hg st
  ? test
  $ rm test

Test dirty working copy and --merge.
  $ hg up bottom
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark bottom)
  $ echo test >> mf
  $ hg st
  M mf
  $ hg next
  abort: uncommitted changes
  (use --clean to discard uncommitted changes or --merge to bring them along)
  [255]
  $ hg next --merge
  merging mf
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark bottom)
  [*] r1 (glob)
  $ hg st
  M mf
  $ hg up -C .
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Test --newest flag.
  $ hg up 3
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ touch test
  $ hg add test
  $ hg commit -m "test"
  $ hg book other
  $ showgraph
  @  6 other test
  |
  | o  5 top r5
  | |
  | o  4  r4
  |/
  o  3 bookmark r3
  |
  o  2  r2
  |
  o  1  r1
  |
  o  0 bottom r0
  $ hg up bottom
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (activating bookmark bottom)
  $ hg next --top
  current stack has multiple heads, namely:
  [*] (top) r5 (glob)
  [*] (other) test (glob)
  abort: ambiguous next changeset
  (use the --newest flag to always pick the newest child at each step)
  [255]
  $ hg log -r .
  changeset:   0:fdaccbb26270
  bookmark:    bottom
  user:        debugbuilddag
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     r0
  
  $ hg next --top --newest
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark bottom)
  [*] (other) test (glob)
  (activating bookmark other)

Test --towards flag.
  $ hg up bottom
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (activating bookmark bottom)
  $ showgraph
  o  6 other test
  |
  | o  5 top r5
  | |
  | o  4  r4
  |/
  o  3 bookmark r3
  |
  o  2  r2
  |
  o  1  r1
  |
  @  0 bottom r0
  $ hg next 4 --towards 1
  changeset * has multiple children, namely: (glob)
  [*] r4 (glob)
  [*] (other) test (glob)
  abort: ambiguous next changeset
  (use the --newest or --towards flags to specify which child to pick)
  [255]
  $ hg next 4 --towards 'top+other'
  abort: 'top+other' refers to multiple changesets
  [255]
  $ hg next 4 --towards top
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark bottom)
  [*] r4 (glob)
  $ hg next --towards other
  abort: the current changeset is not an ancestor of 'other'
  [255]

Test next prefer draft commit.
  $ hg up 3
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ showgraph
  o  6 other test
  |
  | o  5 top r5
  | |
  | o  4  r4
  |/
  @  3 bookmark r3
  |
  o  2  r2
  |
  o  1  r1
  |
  o  0 bottom r0
Here we have 2 draft children.
  $ hg next
  changeset * has multiple children, namely: (glob)
  [*] r4 (glob)
  [*] (other) test (glob)
  abort: ambiguous next changeset
  (use the --newest or --towards flags to specify which child to pick)
  [255]
Let's make one of child commits public.
  $ hg phase -p top
Now we have only 1 draft child.
  $ hg next
  changeset * has multiple children, namely: (glob)
  [*] r4 (glob)
  [*] (other) test (glob)
  choosing the only draft child: * (glob)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [*] (other) test (glob)
  (activating bookmark other)
