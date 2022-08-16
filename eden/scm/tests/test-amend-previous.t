#chg-compatible
#debugruntest-compatible

Set up test environment.
  $ configure mutation-norecord
  $ enable amend rebase
  $ setconfig ui.disallowemptyupdate=true
  $ newrepo amendprevious

Check help text for new options and removal of unsupported options.
  $ hg previous --help
  hg previous [OPTIONS]... [STEPS]
  
  aliases: prev
  
  check out the parent commit
  
  Options:
  
      --newest               always pick the newest parent when a changeset has
                             multiple parents
      --bottom               update to the lowest non-public ancestor of the
                             current changeset
      --bookmark             update to the first ancestor with a bookmark
      --no-activate-bookmark do not activate the bookmark on the destination
                             changeset
   -C --clean                discard uncommitted changes (no backup)
   -B --move-bookmark        move active bookmark
   -m --merge                merge uncommitted changes
   -c --check                require clean working directory
  
  (some details hidden, use --verbose to show complete help)

Create stack of commits and go to the top.
  $ hg debugbuilddag --mergeable-file +6
  $ hg up tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg book top

Test invalid argument combinations.
  $ hg previous --bottom 1
  abort: cannot use both number and --bottom
  [255]
  $ hg previous --bookmark 1
  abort: cannot use both number and --bookmark
  [255]
  $ hg previous --bottom --bookmark
  abort: cannot use both --bottom and --bookmark
  [255]

Test basic usage.
  $ hg previous
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark top)
  [*] r4 (glob)

With positional argument.
  $ hg previous 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [*] r2 (glob)

Overshoot bottom of repo.
  $ hg previous 5
  reached root changeset
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [*] r0 (glob)

Test --bottom flag.
  $ hg up top
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark top)
  $ hg previous --bottom
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark top)
  [*] r0 (glob)

Test bookmark navigation.
  $ hg book -r 'desc(r0)' root
  $ hg book -r 'desc(r2)' bookmark
  $ hg up top
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark top)
  $ hg previous --bookmark
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark top)
  [*] (bookmark) r2 (glob)
  (activating bookmark bookmark)
  $ hg previous --bookmark
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark bookmark)
  [*] (root) r0 (glob)
  (activating bookmark root)

Test bookmark activation.
  $ hg up top
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark top)
  $ hg previous 3
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark top)
  [*] (bookmark) r2 (glob)
  (activating bookmark bookmark)
  $ hg previous 2 --no-activate-bookmark
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark bookmark)
  [*] (root) r0 (glob)

Test dirty working copy and --merge.
  $ hg up top
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark top)
  $ echo "test" >> mf
  $ hg st
  M mf
  $ hg previous --check
  abort: uncommitted changes
  [255]
  $ hg previous --merge
  merging mf
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark top)
  [*] r4 (glob)
  $ hg st
  M mf

Test dirty working copy and --clean.
  $ hg previous --check
  abort: uncommitted changes
  [255]
  $ hg previous --clean
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [*] r3 (glob)
  $ hg st

Test multiple parents
  $ hg up 'desc(r3)' -q
  $ echo a > a && hg add a && hg commit -m a
  $ hg merge 'desc(r5)' -q && hg commit -m merge
  $ showgraph
  @    55f23eb33584 merge
  ├─╮
  │ o  8305126fd490 a
  │ │
  o │  f2987ebe5838 r5
  │ │
  o │  aa70f0fe546a r4
  ├─╯
  o  cb14eba0ad9c r3
  │
  o  f07e66f449d0 r2
  │
  o  09bb8c08de89 r1
  │
  o  fdaccbb26270 r0
  $ hg previous
  changeset 55f23eb33584 has multiple parents, namely:
  [f2987e] (top) r5
  [830512] a
  abort: ambiguous previous changeset
  (use the --newest flag to always pick the newest parent at each step)
  [255]
  $ hg --config ui.interactive=true previous 3 <<EOF
  > 1
  > EOF
  changeset 55f23eb33584 has multiple parents, namely:
  (1) [f2987e] (top) r5
  (2) [830512] a
  which changeset to move to [1-2/(c)ancel]?  1
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [cb14eb] r3
  $ hg up 'desc(merge)' -q
  $ hg --config ui.interactive=true previous 3 <<EOF
  > 2
  > EOF
  changeset 55f23eb33584 has multiple parents, namely:
  (1) [f2987e] (top) r5
  (2) [830512] a
  which changeset to move to [1-2/(c)ancel]?  2
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [f07e66] (bookmark) r2
  (activating bookmark bookmark)

Mix with bottom:
  $ hg debugmakepublic 'desc(r4)'
  $ hg up 'desc(merge)' -q
  $ hg prev --bottom
  current stack has multiple bottom changesets, namely:
  [f2987e] (top) r5
  [830512] a
  abort: ambiguous bottom changeset
  [255]
  $ hg --config ui.interactive=true previous --bottom <<EOF
  > 2
  > EOF
  current stack has multiple bottom changesets, namely:
  (1) [f2987e] (top) r5
  (2) [830512] a
  which changeset to move to [1-2/(c)ancel]?  2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [830512] a
