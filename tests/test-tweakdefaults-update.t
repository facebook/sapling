  $ extpath=`dirname $TESTDIR`
  $ cp $extpath/hgext3rd/tweakdefaults.py $TESTTMP # use $TESTTMP substitution in message
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > tweakdefaults=$TESTTMP/tweakdefaults.py
  > rebase=
  > EOF

Set up the repository.
  $ hg init repo
  $ cd repo
  $ hg debugbuilddag -m '+4 *3 +1'
  $ hg log --graph -r 0:: -T '{rev}'
  o  5
  |
  o  4
  |
  | o  3
  | |
  | o  2
  |/
  o  1
  |
  o  0
  
  $ hg up 3
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Make an uncommitted change.
  $ echo foo > foo
  $ hg add foo
  $ hg st
  A foo

Can always update to current commit.
  $ hg up .
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

By default, --check should be set.
  $ hg up 2
  abort: uncommitted changes
  [255]
  $ hg up --nocheck 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Updates to other branches should fail without --merge.
  $ hg up 4
  abort: uncommitted changes
  [255]
  $ hg up --nocheck 4
  abort: uncommitted changes
  (commit or update --clean to discard changes)
  [255]
  $ hg up --merge 4
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Certain flags shouldn't work together.
  $ hg up --check --merge 3
  abort: can only specify one of -C/--clean, -c/--check, or -m/merge
  [255]
  $ hg up --check --clean 3
  abort: can only specify one of -C/--clean, -c/--check, or -m/merge
  [255]
  $ hg up --clean --merge 3
  abort: can only specify one of -C/--clean, -c/--check, or -m/merge
  [255]

--clean should work as expected.
  $ hg st
  A foo
  $ hg up --clean 3
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg st
  ? foo
