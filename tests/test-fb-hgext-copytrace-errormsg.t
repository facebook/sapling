TODO: Make this test compatibile with obsstore enabled.
  $ setconfig experimental.evolution=

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > rebase=
  > copytrace=
  > [experimental]
  > copytrace=off
  > EOF

  $ hg init repo
  $ cd repo
  $ echo 1 > 1
  $ hg add 1
  $ hg ci -m 1
  $ echo 2 > 1
  $ hg ci -m 2
  $ hg up 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg mv 1 2
  $ hg ci -m dest
  $ hg rebase -s 1 -d .
  rebasing 1:812796267395 "2"
  other [source] changed 1 which local [dest] deleted
  hint: if this message is due to a moved file, you can ask mercurial to attempt to automatically resolve this change by re-running with the --tracecopies flag, but this will significantly slow down the operation, so you will need to be patient.
  Source control team is working on fixing this problem.
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved? u
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ hg rebase --abort
  rebase aborted
  $ hg rebase -s 1 -d . --tracecopies
  rebasing 1:812796267395 "2"
  merging 2 and 1 to 2
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/812796267395-81e11405-rebase.hg (glob)
