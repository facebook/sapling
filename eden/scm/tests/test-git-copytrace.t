#require git no-windows
#debugruntest-compatible

  $ eagerepo
  $ . $TESTDIR/git.sh
  $ setconfig diff.git=True
  $ enable rebase copytrace

  $ setupconfig() {
  >   setconfig copytrace.fastcopytrace=True
  >   setconfig copytrace.dagcopytrace=True
  > }

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

Test missing files in source side

  $ hg init --git repo2
  $ cd repo2
  $ setupconfig
  $ drawdag <<'EOS'
  > C   # C/y = 1\n (renamed from x)
  > |   # C/C = (removed)
  > |
  > | B # B/x = 1\n2\n
  > | | # B/B = (removed)
  > |/
  > A   # A/x = 1\n
  >     # A/A = (removed)
  > EOS

fixme(zhaolong): this is a bug, copytrace should resolve the rename conflict

  $ hg rebase -r $C -d $B
  rebasing 470d2f079ab1 "C"
  local [dest] changed x which other [source] deleted
  hint: if this message is due to a moved file, you can ask mercurial to attempt to automatically resolve this change by re-running with the --config=experimental.copytrace=on flag, but this will significantly slow down the operation, so you will need to be patient.
  Source control team is working on fixing this problem.
  use (c)hanged version, (d)elete, or leave (u)nresolved? u
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
