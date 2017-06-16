Set up test environment.
  $ . $TESTDIR/require-ext.sh evolve
  $ extpath=`dirname $TESTDIR`
  $ cp $extpath/hgext3rd/allowunstable.py $TESTTMP # use $TESTTMP substitution in message
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > allowunstable=$TESTTMP/allowunstable.py
  > directaccess=$TESTDIR/../hgext3rd/directaccess.py
  > evolve=
  > histedit=
  > inhibit=$TESTDIR/../hgext3rd/inhibit.py
  > rebase=
  > record=
  > [experimental]
  > evolution = createmarkers
  > [ui]
  > interactive = true
  > EOF
  $ showgraph() {
  >   hg log -r '(::.)::' --graph -T "{rev} {desc|firstline}" | sed \$d
  > }
  $ reset() {
  >   cd ..
  >   rm -rf allowunstable
  >   hg init allowunstable
  >   cd allowunstable
  > }
  $ hg init allowunstable && cd allowunstable
  $ hg debugbuilddag +5

Test that we can perform a splits and histedits in the middle of a stack.
Since these are interactive commands, just ensure that we don't get
an error message.
  $ hg up 2
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg split
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg histedit

Test that we can perform a fold in the middle of a stack.
  $ hg up 2
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg fold --from ".^"
  2 changesets folded
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ showgraph
  @  5 r1
  |
  | o  4 r4
  | |
  | o  3 r3
  | |
  | o  2 r2
  | |
  | o  1 r1
  |/
  o  0 r0

Test that we can perform a rebase in the middle of a stack.
  $ hg rebase -r 3 -d 5
  rebasing 3:2dc09a01254d "r3"
  note: rebase of 3:2dc09a01254d created no changes to commit
  1 new unstable changesets
  $ showgraph
  @  5 r1
  |
  | o  4 r4
  | |
  | x  3 r3
  | |
  | o  2 r2
  | |
  | o  1 r1
  |/
  o  0 r0

Test that we can perform `hg record --amend` in the middle of a stack.
  $ reset
  $ hg debugbuilddag +3
  $ hg up 1
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ touch foo
  $ hg add foo
  $ hg record --amend << EOF
  > y
  > EOF
  diff --git a/foo b/foo
  new file mode 100644
  examine changes to 'foo'? [Ynesfdaq?] y
  
  $ showgraph
  @  4 r1
  |
  | o  2 r2
  | |
  | o  1 r1
  |/
  o  0 r0
