#chg-compatible

Test new conflict switching:

  $ configure mutation-norecord
  $ newrepo
  $ enable amend morestatus rebase
  $ setconfig morestatus.show=True
  $ setconfig rebase.singletransaction=True
  $ setconfig rebase.experimental.inmemory=True
  $ setconfig rebase.experimental.inmemorywarning="rebasing in-memory!"

  $ hg debugdrawdag <<'EOS'
  >   f
  >   |
  >   e
  >   |
  > c d
  > |/
  > b g
  > |/
  > a
  > EOS

Make conflicts halfway up the stack:
  $ hg up -C f
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark f)
  $ echo "conflict" > c
  $ hg add c
  $ hg amend -q
  $ hg rebase -q -s g -d .
  rebasing in-memory!
  $ hg log -G -r 0:: -T '{desc}'
  o  g
  |
  @  f
  |
  o  e
  |
  o  d
  |
  | o  c
  |/
  o  b
  |
  o  a
  
  $ cp -R . ../control
  $ hg rebase -d c
  rebasing in-memory!
  rebasing f4016ed9f5d0 "d" (d)
  rebasing 881eb15e0fdf "e" (e)
  rebasing e692c3b32196 "f" (f)
  merging c
  hit merge conflicts (in c); switching to on-disk merge
  rebasing e692c3b32196 "f" (f)
  merging c
  warning: 1 conflicts while merging c! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ hg resolve --all --tool :other
  (no more unresolved files)
  continue: hg rebase --continue
  $ hg rebase --continue
  already rebased f4016ed9f5d0 "d" (d) as 32bb4413a7df
  already rebased 881eb15e0fdf "e" (e) as d82c41319fdd
  rebasing e692c3b32196 "f" (f)
  rebasing 2a19607ff85c "g" (g)
  $ hg log -G -r 0:: -T '{desc} {rev} {node|short}'
  o  g 12 24c12a3229e2
  |
  @  f 11 c33e7f678afd
  |
  o  e 10 d82c41319fdd
  |
  o  d 9 32bb4413a7df
  |
  o  c 3 a82ac2b38757
  |
  o  b 1 488e1b7e7341
  |
  o  a 0 b173517d0057
  

Try it with uncommitted changes, ensure it aborts nicely:

  $ hg up -Cq a
  $ hg clean
  $ echo "test" > a
  $ hg rebase -s d82c41319fdd -d a
  rebasing in-memory!
  rebasing d82c41319fdd "e" (e)
  rebasing c33e7f678afd "f" (f)
  transaction abort!
  rollback completed
  abort: must use on-disk merge for this rebase (hit merge conflicts in c), but you have working copy changes
  (commit, revert, or shelve them)
  [255]
  $ hg st
  M a
  $ cat a
  test
