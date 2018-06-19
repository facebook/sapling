Test new conflict switching:

  $ newrepo
  $ enable rebase purge fbamend morestatus
  $ setconfig morestatus.show=True
  $ setconfig rebase.experimental.inmemory=True
  $ setconfig rebase.experimental.inmemory.nomergedriver=False
  $ setconfig rebase.experimental.inmemory.canrebaseworkingcopy=True
  $ setconfig rebase.experimental.inmemory.newconflictswitching=True
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
  rebasing 3:f4016ed9f5d0 "d" (d)
  rebasing 4:881eb15e0fdf "e" (e)
  rebasing 5:e692c3b32196 "f"
  merging c
  hit merge conflicts; switching to on-disk merge (in-memory merge does not support merge conflicts)
  rebasing 5:e692c3b32196 "f"
  merging c
  warning: conflicts while merging c! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ hg resolve --all --tool :other
  (no more unresolved files)
  continue: hg rebase --continue
  $ hg rebase --continue
  already rebased 3:f4016ed9f5d0 "d" (d) as 32bb4413a7df
  already rebased 4:881eb15e0fdf "e" (e) as d82c41319fdd
  rebasing 5:e692c3b32196 "f"
  rebasing 6:2a19607ff85c "g"
  saved backup bundle to $TESTTMP/repo1/.hg/strip-backup/f4016ed9f5d0-8f1f8064-rebase.hg
  $ hg log -G -r 0:: -T '{desc} {rev} {node|short}'
  o  g 6 24c12a3229e2
  |
  @  f 5 c33e7f678afd
  |
  o  e 4 d82c41319fdd
  |
  o  d 3 32bb4413a7df
  |
  o  c 2 a82ac2b38757
  |
  o  b 1 488e1b7e7341
  |
  o  a 0 b173517d0057
  

Rerun it without the config and confirm the created hashes are the same:

  $ cd ../control
  $ setconfig rebase.experimental.inmemory.newconflictswitching=False
  $ hg rebase -d c
  rebasing in-memory!
  rebasing 3:f4016ed9f5d0 "d" (d)
  rebasing 4:881eb15e0fdf "e" (e)
  rebasing 5:e692c3b32196 "f"
  merging c
  hit merge conflicts; using on-disk merge instead (in-memory merge does not support merge conflicts)
  rebasing 3:f4016ed9f5d0 "d" (d)
  rebasing 4:881eb15e0fdf "e" (e)
  rebasing 5:e692c3b32196 "f"
  merging c
  warning: conflicts while merging c! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ hg resolve --all --tool :other
  (no more unresolved files)
  continue: hg rebase --continue
  $ hg rebase --continue
  already rebased 3:f4016ed9f5d0 "d" (d) as 32bb4413a7df
  already rebased 4:881eb15e0fdf "e" (e) as d82c41319fdd
  rebasing 5:e692c3b32196 "f"
  rebasing 6:2a19607ff85c "g"
  saved backup bundle to $TESTTMP/control/.hg/strip-backup/f4016ed9f5d0-8f1f8064-rebase.hg
  $ hg log -G -r 0:: -T '{desc} {rev} {node|short}'
  o  g 6 24c12a3229e2
  |
  @  f 5 c33e7f678afd
  |
  o  e 4 d82c41319fdd
  |
  o  d 3 32bb4413a7df
  |
  o  c 2 a82ac2b38757
  |
  o  b 1 488e1b7e7341
  |
  o  a 0 b173517d0057
  
Try it with uncommitted changes, ensure it aborts nicely:

  $ setconfig rebase.experimental.inmemory.newconflictswitching=True
  $ hg up -Cq a
  $ hg clean
  $ echo "test" > a
  $ hg rebase -s d82c41319fdd -d a
  rebasing in-memory!
  rebasing 4:d82c41319fdd "e"
  rebasing 5:c33e7f678afd "f"
  abort: must use on-disk merge for this rebase (in-memory merge does not support file conflicts), but you have working copy changes
  (commit, revert, or shelve them)
  [255]
  $ hg st
  M a
  $ cat a
  test

Rerun the last test with the old config:

  $ setconfig rebase.experimental.inmemory.newconflictswitching=False
  $ hg rebase -s d82c41319fdd -d a
  rebasing in-memory!
  rebasing 4:d82c41319fdd "e"
  rebasing 5:c33e7f678afd "f"
  hit merge conflicts; using on-disk merge instead (in-memory merge does not support file conflicts)
  abort: uncommitted changes
  (commit, shelve or remove them, then rerun the rebase)
  [255]
  $ hg st
  M a
