Set up test environment.
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > amend=
  > rebase=
  > [experimental]
  > evolution = createmarkers
  > [ui]
  > disallowemptyupdate = True
  > EOF
  $ hg init amendprevious && cd amendprevious

Check help text for new options and removal of unsupported options.
  $ hg previous --help
  hg previous [OPTIONS]... [STEPS]
  
  check out the parent commit
  
  Options:
  
      --clean                discard uncommitted changes (no backup)
      --newest               always pick the newest parent when a changeset has
                             multiple parents
      --bottom               update to the lowest non-public ancestor of the
                             current changeset
      --bookmark             update to the first ancestor with a bookmark
      --no-activate-bookmark do not activate the bookmark on the destination
                             changeset
   -B --move-bookmark        move active bookmark
      --merge                merge uncommitted changes
  
  (some details hidden, use --verbose to show complete help)

Create stack of commits and go to the top.
  $ hg debugbuilddag +6
  $ hg up c8d03c
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
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
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark top)
  [bebd16] r4

With positional argument.
  $ hg previous 2
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [012414] r2

Overshoot bottom of repo.
  $ hg previous 5
  reached root changeset
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [1ea734] r0

Test --bottom flag.
  $ hg up top
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark top)
  $ hg previous --bottom
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark top)
  [1ea734] r0

Test bookmark navigation.
  $ hg book -r 1ea734 root
  $ hg book -r 012414 bookmark
  $ hg up top
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark top)
  $ hg previous --bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark top)
  [012414] (bookmark) r2
  (activating bookmark bookmark)
  $ hg previous --bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark bookmark)
  [1ea734] (root) r0
  (activating bookmark root)

Test bookmark activation.
  $ hg up top
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark top)
  $ hg previous 3
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark top)
  [012414] (bookmark) r2
  (activating bookmark bookmark)
  $ hg previous 2 --no-activate-bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark bookmark)
  [1ea734] (root) r0

Test dirty working copy and --merge.
  $ hg up top
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark top)
  $ touch test
  $ hg add test
  $ hg st
  A test
  $ hg previous
  abort: uncommitted changes
  (use --clean to discard uncommitted changes or --merge to bring them along)
  [255]
  $ hg previous --merge
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark top)
  [bebd16] r4
  $ hg st
  A test

Test dirty working copy and --clean.
  $ hg previous
  abort: uncommitted changes
  (use --clean to discard uncommitted changes or --merge to bring them along)
  [255]
  $ hg previous --clean
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [2dc09a] r3
  $ hg st
  ? test
