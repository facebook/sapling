#chg-compatible

Test new conflict switching:

  $ newrepo
  $ enable amend morestatus purge rebase
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
  | x  f
  |/
  o  e
  |
  o  d
  |
  | o  c
  |/
  | x  g
  | |
  o |  b
  |/
  o  a
  
  $ cp -R . ../control
  $ hg rebase -d c
  rebasing in-memory!
  rebasing f4016ed9f5d0 "d" (d)
  rebasing 881eb15e0fdf "e" (e)
  note: not rebasing 22d86c9ba040 "f" (f) and its descendants as this would cause divergence
  rebasing e692c3b32196 "f"
  merging c
  hit merge conflicts (in c); switching to on-disk merge
  rebasing e692c3b32196 "f"
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
  note: not rebasing 22d86c9ba040 "f" (f) and its descendants as this would cause divergence
  rebasing e692c3b32196 "f"
  rebasing 2a19607ff85c "g"
  $ hg log -G -r 0:: -T '{desc} {rev} {node|short}'
  o  g 12 24c12a3229e2
  |
  @  f 11 c33e7f678afd
  |
  o  e 10 d82c41319fdd
  |
  o  d 9 32bb4413a7df
  |
  | x  f 6 22d86c9ba040
  | |
  | x  e 5 881eb15e0fdf
  | |
  | x  d 4 f4016ed9f5d0
  | |
  o |  c 3 a82ac2b38757
  |/
  | x  g 2 cf64e78ac512
  | |
  o |  b 1 488e1b7e7341
  |/
  o  a 0 b173517d0057
  

Try it with uncommitted changes, ensure it aborts nicely:

  $ hg up -Cq a
  $ hg clean
  $ echo "test" > a
  $ hg rebase -s d82c41319fdd -d a
  rebasing in-memory!
  rebasing d82c41319fdd "e"
  rebasing c33e7f678afd "f"
  transaction abort!
  rollback completed
  abort: must use on-disk merge for this rebase (hit merge conflicts in c), but you have working copy changes
  (commit, revert, or shelve them)
  [255]
  $ hg st
  M a
  $ cat a
  test
