#chg-compatible

  $ setconfig workingcopy.ruststatus=False
  $ setconfig status.use-rust=False workingcopy.use-rust=False
  $ configure mutation-norecord
Tests the --noconflict rebase flag

  $ enable amend morestatus rebase
  $ setconfig morestatus.show=True
  $ setconfig rebase.singletransaction=True
  $ setconfig rebase.experimental.inmemory=True
  $ setconfig rebase.experimental.inmemorywarning="rebasing in-memory!"
  $ newrepo

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
  (activating bookmark c)
  $ echo "local change" > b


Confirm it fails when rebasing a change that conflicts:
  $ hg rebase -r tip -d . --noconflict
  rebasing in-memory!
  rebasing 955ac081fc7c "g" (g)
  merging c
  hit merge conflicts (in c) and --noconflict passed; exiting
  $ hg st
  M b
  $ cat b
  local change

Confirm rebase without a merge behaves the same:
  $ hg rebase -r tip -d .~1 --noconflict
  rebasing in-memory!
  rebasing 955ac081fc7c "g" (g)

Confirm the flag fails without IMM:

  $ setconfig rebase.experimental.inmemory=False
  $ hg up -C .
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg st
  $ hg rebase -r tip -d . --noconflict
  abort: --noconflict requires in-memory merge
  [255]

Confirm that it rebases a three-way merge, but no conflict:
  $ newrepo
  $ seq 1 5 > a
  $ hg commit -Aq -m "base"
  $ seq 1 10 > a
  $ hg commit -q -m "extend to 10"
  $ hg up -q .~1
  $ seq 0 5 > a
  $ hg commit -q -m "prepend with 0"
  $ hg log -G -r 'desc(base)':: -T '{desc}'
  @  prepend with 0
  │
  │ o  extend to 10
  ├─╯
  o  base
  
  $ hg up -qC 'desc(base)'
  $ hg rebase -r 'desc(extend)' -d 'desc(prepend)' --noconflict
  rebasing in-memory!
  rebasing 12cba56c6d27 "extend to 10"
  merging a
  $ hg cat -r tip a | wc -l | xargs
  11

^ (xargs is used for trimming)
