#chg-compatible
#debugruntest-compatible

Set up test environment.
  $ configure mutation-norecord
  $ enable amend rebase
  $ showgraph() {
  >   hg log --graph -T "{bookmarks} {desc|firstline}" | sed \$d
  > }
  $ newrepo

Check help text for new options and removal of unsupported options.
  $ hg next --help
  hg next [OPTIONS]... [STEPS]
  
  aliases: n
  
  check out a descendant commit
  
      Update to a descendant commit of the current commit. When working with a
      stack of commits, you can use 'hg next' to move up your stack with ease.
  
      - Use the "--newest" flag to always pick the newest of multiple child
        commits. You can set "amend.alwaysnewest" to true in your global
        Mercurial config file to make this the default.
      - Use the "--merge" flag to bring along uncommitted changes to the
        destination commit.
      - Use the "--bookmark" flag to move to the next commit with a bookmark.
      - Use the "--rebase" flag to rebase any child commits that were left
        behind after "amend", "split", "fold", or "histedit".
  
      Examples:
  
      - Move 1 level up the stack:
  
          hg next
  
      - Move 2 levels up the stack:
  
          hg next 2
  
      - Move to the top of the stack:
  
          hg next --top
  
  Options:
  
      --newest               always pick the newest child when a commit has
                             multiple children
      --rebase               rebase each commit if necessary
      --top                  update to the head of the current stack
      --bookmark             update to the first commit with a bookmark
      --no-activate-bookmark do not activate the bookmark on the destination
                             commit
      --towards VALUE        move linearly towards the specified head
   -C --clean                discard uncommitted changes (no backup)
   -B --move-bookmark        move active bookmark
   -m --merge                merge uncommitted changes
   -c --check                require clean working directory
  
  (some details hidden, use --verbose to show complete help)
Create stack of commits and go to the bottom.
  $ hg debugbuilddag --mergeable-file +6
  $ hg up 'desc(r0)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg book bottom
  $ showgraph
  o   r5
  │
  o   r4
  │
  o   r3
  │
  o   r2
  │
  o   r1
  │
  @  bottom r0

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
  $ hg book -r 'desc(r5)' top
  $ hg book -r 'desc(r3)' bookmark
  $ showgraph
  @  top r5
  │
  o   r4
  │
  o  bookmark r3
  │
  o   r2
  │
  o   r1
  │
  o  bottom r0
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
  $ hg next --check
  abort: uncommitted changes
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
  $ hg next --check
  abort: uncommitted changes
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
  $ hg up 'desc(r3)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ touch test
  $ hg add test
  $ hg commit -m "test"
  $ hg book other
  $ showgraph
  @  other test
  │
  │ o  top r5
  │ │
  │ o   r4
  ├─╯
  o  bookmark r3
  │
  o   r2
  │
  o   r1
  │
  o  bottom r0
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
  commit:      fdaccbb26270
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
  o  other test
  │
  │ o  top r5
  │ │
  │ o   r4
  ├─╯
  o  bookmark r3
  │
  o   r2
  │
  o   r1
  │
  @  bottom r0
  $ hg next 4 --towards 'desc(r1)'
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

Test interactive:
  $ hg up 'desc(test)' -q && touch a && hg add a && hg commit -m "branch a"
  $ hg up 'desc(test)' -q && touch b && hg add b && hg commit -m "branch b"
  $ hg up bottom -q
  $ showgraph
  o   branch b
  │
  │ o   branch a
  ├─╯
  o  other test
  │
  │ o  top r5
  │ │
  │ o   r4
  ├─╯
  o  bookmark r3
  │
  o   r2
  │
  o   r1
  │
  @  bottom r0
  $ hg --config ui.interactive=true next 5 <<EOF
  > 2
  > 1
  > EOF
  changeset cb14eba0ad9c has multiple children, namely:
  (1) [aa70f0] r4
  (2) [2341c6] (other) test
  which changeset to move to [1-2/(c)ancel]?  2
  changeset 2341c6305f4b has multiple children, namely:
  (1) [ae9b2b] branch a
  (2) [9913ce] branch b
  which changeset to move to [1-2/(c)ancel]?  1
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark bottom)
  [ae9b2b] branch a
  $ hg up bottom -q
  $ hg --config ui.interactive=true next --top <<EOF
  > 3
  > EOF
  current stack has multiple heads, namely:
  (1) [f2987e] (top) r5
  (2) [ae9b2b] branch a
  (3) [9913ce] branch b
  which changeset to move to [1-3/(c)ancel]?  3
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark bottom)
  [9913ce] branch b

Test interactive >= 10 choices:
  $ drawdag << 'EOS'
  > a b c   g h i
  >  \|/     \|/
  >   | d e f | j
  >    \ \|/ / /
  >   desc('b')
  > EOS
  $ hg --config ui.interactive=true next << EOS
  > 10
  > EOS
  changeset 9913ce0137a4 has multiple children, namely:
  (1) [3f9bda] a
  (2) [64b9b8] b
  (3) [95297f] c
  (4) [551771] d
  (5) [f44bd1] e
  (6) [f72cbe] f
  (7) [60b350] g
  (8) [f09214] h
  (9) [23284c] i
  (10) [1e290b] j
  which changeset to move to [1-10/(c)ancel]?  10
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [1e290b] j
  $ hg up bottom -q
  $ hg --config ui.interactive=true next --top << EOS
  > 10
  > EOS
  current stack has multiple heads, namely:
  (1) [f2987e] (top) r5
  (2) [ae9b2b] branch a
  (3) [3f9bda] a
  (4) [64b9b8] b
  (5) [95297f] c
  (6) [551771] d
  (7) [f44bd1] e
  (8) [f72cbe] f
  (9) [60b350] g
  (10) [f09214] h
  (11) [23284c] i
  (12) [1e290b] j
  which changeset to move to [1-12/(c)ancel]?  10
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark bottom)
  [f09214] h


Test next prefer draft commit.
  $ hg up 'desc(r3)' -q
  $ hg log -Gr '.+children(.)' -T '{desc}'
  o  test
  │
  │ o  r4
  ├─╯
  @  r3
  │
  ~
Here we have 2 draft children.
  $ hg next
  changeset * has multiple children, namely: (glob)
  [*] r4 (glob)
  [*] (other) test (glob)
  abort: ambiguous next changeset
  (use the --newest or --towards flags to specify which child to pick)
  [255]
Let's make one of child commits public.
  $ hg debugmakepublic top
Now we have only 1 draft child.
  $ hg next
  changeset * has multiple children, namely: (glob)
  [*] r4 (glob)
  [*] (other) test (glob)
  choosing the only draft child: * (glob)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [*] (other) test (glob)
  (activating bookmark other)
