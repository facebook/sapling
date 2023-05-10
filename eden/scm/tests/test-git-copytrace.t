#require git no-windows
#debugruntest-compatible

  $ eagerepo
  $ . $TESTDIR/git.sh
  $ setconfig diff.git=True

Prepare repo

  $ hg init --git repo1
  $ cd repo1
  $ cat > a << EOF
  > 1
  > 2
  > 3
  > 4
  > 5
  > EOF
  $ hg ci -q -Am 'add a'

Test copytrace

  $ hg rm a
  $ cat > b << EOF
  > 1
  > 2
  > 3
  > 4
  > EOF
  $ hg ci -q -Am 'mv a -> b'

Default similarity threshold 0.8 should work

  $ hg debugcopytrace -s .~1 -d . a
  {"a": "b"}

High similarity threshold should fail to find the rename
  $ hg debugcopytrace -s .~1 -d . a --config copytrace.similarity-threshold=0.91
  {"a": null}

Low max rename edit cost should fail to find the rename
  $ hg debugcopytrace -s .~1 -d . a --config copytrace.max-edit-cost=0
  {"a": null}
