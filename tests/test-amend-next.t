Set up test environment.
  $ enable amend obsstore rebase
  $ showgraph() {
  >   hg log --graph -T "{rev} {bookmarks} {desc|firstline}" | sed \$d
  > }
  $ newrepo

Check help text for new options and removal of unsupported options.
  $ hg next --help
  hg next [OPTIONS]... [STEPS]
  
  update to child changeset
  
  Options:
  
      --clean                discard uncommitted changes (no backup)
      --newest               always pick the newest child when a changeset has
                             multiple children
      --rebase               rebase each changeset if necessary
      --top                  update to the head of the current stack
      --bookmark             update to the first changeset with a bookmark
      --no-activate-bookmark do not activate the bookmark on the destination
                             changeset
      --towards VALUE        move linearly towards the specified head
   -B --move-bookmark        move active bookmark
      --merge                merge uncommitted changes
  
  (some details hidden, use --verbose to show complete help)

Create stack of commits and go to the bottom.
  $ hg debugbuilddag +6
  $ hg up 1ea734
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
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
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark bottom)
  [66f7d4] r1

With positional argument.
  $ hg next 2
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [2dc09a] r3

Overshoot top of repo.
  $ hg next 5
  reached head changeset
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [c8d03c] r5

Test --top flag.
  $ hg up bottom
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark bottom)
  $ hg next --top
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark bottom)
  [c8d03c] r5

Test bookmark navigation.
  $ hg book -r c8d03c top
  $ hg book -r 2dc09a bookmark
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
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark bottom)
  $ hg next --bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark bottom)
  [2dc09a] (bookmark) r3
  (activating bookmark bookmark)
  $ hg next --bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark bookmark)
  [c8d03c] (top) r5
  (activating bookmark top)

Test bookmark activation.
  $ hg up bottom
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark bottom)
  $ hg next 3
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark bottom)
  [2dc09a] (bookmark) r3
  (activating bookmark bookmark)
  $ hg next 2 --no-activate-bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark bookmark)
  [c8d03c] (top) r5

Test dirty working copy and --clean.
  $ hg up bottom
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
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
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark bottom)
  [66f7d4] r1
  $ hg st
  ? test
  $ rm test

Test dirty working copy and --merge.
  $ hg up bottom
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark bottom)
  $ touch test
  $ hg add test
  $ hg st
  A test
  $ hg next
  abort: uncommitted changes
  (use --clean to discard uncommitted changes or --merge to bring them along)
  [255]
  $ hg next --merge
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark bottom)
  [66f7d4] r1
  $ hg st
  A test
  $ hg forget test

Test --newest flag.
  $ hg up 3
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
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
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (activating bookmark bottom)
  $ hg next --top
  current stack has multiple heads, namely:
  [c8d03c] (top) r5
  [10f4a7] (other) test
  abort: ambiguous next changeset
  (use the --newest flag to always pick the newest child at each step)
  [255]
  $ hg log -r .
  changeset:   0:1ea73414a91b
  bookmark:    bottom
  user:        debugbuilddag
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     r0
  
  $ hg next --top --newest
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark bottom)
  [10f4a7] (other) test
  (activating bookmark other)

Test --towards flag.
  $ hg up bottom
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
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
  changeset 2dc09a01254d has multiple children, namely:
  [bebd16] r4
  [10f4a7] (other) test
  abort: ambiguous next changeset
  (use the --newest or --towards flags to specify which child to pick)
  [255]
  $ hg next 4 --towards 'top+other'
  abort: 'top+other' refers to multiple changesets
  [255]
  $ hg next 4 --towards top
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark bottom)
  [bebd16] r4
  $ hg next --towards other
  abort: the current changeset is not an ancestor of 'other'
  [255]
