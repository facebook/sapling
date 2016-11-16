Set up test environment.
  $ . $TESTDIR/require-ext.sh directaccess evolve inhibit
  $ extpath=`dirname $TESTDIR`
  $ cp $extpath/hgext3rd/fbamend.py $TESTTMP # use $TESTTMP substitution in message
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > directaccess=
  > evolve=
  > fbamend=$TESTTMP/fbamend.py
  > inhibit=
  > rebase=
  > [experimental]
  > evolution = createmarkers
  > evolutioncommands = previous next
  > EOF
  $ hg init fbamendprevious && cd fbamendprevious

Check help text for new options and removal of unsupported options.
  $ hg previous --help
  hg previous [OPTION]... [NUM_STEPS]
  
  update to parent revision
  
      Displays the summary line of the destination for clarity.
  
  options:
  
   -B --move-bookmark        move active bookmark after update
      --merge                bring uncommitted change along
      --newest               always pick the newest parent when a changeset has
                             multiple parents
      --bottom               update to the lowest non-public ancestor of the
                             current changeset
      --bookmark             update to the first ancestor with a bookmark
      --no-activate-bookmark do not activate the bookmark on the destination
                             changeset
  
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
  [1ea734] r0

Test bookmark navigation.
  $ hg book -r 1ea734 root
  $ hg book -r 012414 bookmark
  $ hg up top
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg previous --bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark top)
  [012414] (bookmark) r2
  (activating bookmark bookmark)
  $ hg previous --bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
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
  (use --merge to bring along uncommitted changes)
  [255]
  $ hg previous --merge
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark top)
  [bebd16] r4
  $ hg st
  A test
