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
  $ hg init fbamendnext && cd fbamendnext

Check help text for new options and removal of unsupported options.
  $ hg next --help
  hg next [OPTION]... [NUM_STEPS]
  
  update to next child revision
  
      Use the "--evolve" flag to evolve unstable children on demand.
  
      Displays the summary line of the destination for clarity.
  
  options:
  
   -B --move-bookmark        move active bookmark after update
      --merge                bring uncommitted change along
      --newest               always pick the newest child when a changeset has
                             multiple children
      --rebase               rebase each changeset if necessary
      --top                  update to the head of the current stack
      --bookmark             update to the first changeset with a bookmark
      --no-activate-bookmark do not activate the bookmark on the destination
                             changeset
  
  (some details hidden, use --verbose to show complete help)

Create stack of commits and go to the bottom.
  $ hg debugbuilddag +6
  $ hg up 1ea734
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg book bottom

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
  (use --merge to bring along uncommitted changes)
  [255]
  $ hg next --merge
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark bottom)
  [66f7d4] r1
  $ hg st
  A test

Test --newest flag.
  $ hg up bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark bookmark)
  $ touch test
  $ hg add test
  test already tracked!
  $ hg commit -m "test"
  created new head
  $ hg up bottom
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (activating bookmark bottom)
  $ hg next --top
  current stack has multiple heads, namely:
  [c8d03c] (top) r5
  [10f4a7] (bookmark) test
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
  [10f4a7] (bookmark) test
  (activating bookmark bookmark)
