Tests the :abort merge tool

  $ newrepo
  $ enable rebase fbamend morestatus
  $ setconfig morestatus.show=True
  $ setconfig rebase.singletransaction=True
  $ setconfig rebase.experimental.inmemory=True
  $ setconfig rebase.experimental.inmemory.nomergedriver=False
  $ setconfig rebase.experimental.inmemory.newconflictswitching=True
  $ setconfig rebase.experimental.inmemorywarning="rebasing in-memory!"

  $ hg debugdrawdag <<'EOS'
  > c
  > |
  > b g
  > |/
  > a
  > EOS
  $ hg up -q g
  $ echo "conflict" > c
  $ hg add -q
  $ hg amend -q
  $ hg up c
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo "local change" > b
  $ hg rebase -r tip -d . --tool :abort
  rebasing in-memory!
  rebasing 3:955ac081fc7c "g" (tip)
  hit merge conflicts, and --tool :abort passed; exiting.
  $ hg st
  M b
  $ cat b
  local change

A rebase without a conflict behaves the same:
  $ hg rebase -r tip -d .~1 --tool :abort
  rebasing in-memory!
  rebasing 3:955ac081fc7c "g" (tip)
  saved backup bundle to $TESTTMP/repo1/.hg/strip-backup/955ac081fc7c-77e57574-rebase.hg

It fails without IMM:

  $ setconfig rebase.experimental.inmemory=False
  $ hg up -C .
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg st
  $ hg rebase -r tip -d . --tool :abort
  rebasing 3:20b9638feb86 "g" (tip)
  transaction abort!
  rollback completed
  abort: --tool :abort only works with in-memory merge
  [255]
