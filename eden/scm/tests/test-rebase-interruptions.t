#modern-config-incompatible

#require no-eden

#chg-compatible


  $ enable rebase

  $ hg init a
  $ cd a
  $ drawdag <<EOS
  > C    # C/A = A\nC\n
  > | E  # E/E = E\n
  > B |  # B/B = B\n
  > | D  # D/A = A\nD\n
  > |/
  > A    # A/A = A\n
  > # drawdag.defaultfiles=false
  > EOS

  $ cd ..

  $ function save_off_rebasestate() {
  >   mv $(hg root)/.hg/rebasestate $(hg root)/.hg/rebasestate.bak
  > }

  $ function restore_rebasestate() {
  >   mv $(hg root)/.hg/rebasestate.bak $(hg root)/.hg/rebasestate
  > }

Changes during an interruption - continue:

  $ hg clone -q a a1
  $ cd a1
  $ hg pull -q -r $C -r $E
  $ hg go -q $E

  $ tglog
  @  a0c831b27e4d 'E'
  │
  │ o  9adab7336c0e 'C'
  │ │
  o │  1204d6864fcb 'D'
  │ │
  │ o  27547f69f254 'B'
  ├─╯
  o  4a2df7238c3b 'A'
Rebasing B onto E:

  $ hg rebase -s 'desc(B)' -d 'desc(E)'
  rebasing 27547f69f254 "B"
  rebasing 9adab7336c0e "C"
  merging A
  warning: 1 conflicts while merging A! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

Force a commit on C during the interruption:

  $ save_off_rebasestate
  $ hg up -q -C 'desc(C)'
  $ restore_rebasestate

  $ echo 'Extra' > Extra
  $ hg add Extra
  $ save_off_rebasestate
  $ hg ci -m 'Extra'
  $ restore_rebasestate

  $ tglogp
  @  a1192b4e4efb draft 'Extra'
  │
  │ o  8e6c056dc407 draft 'B'
  │ │
  │ o  a0c831b27e4d draft 'E'
  │ │
  o │  9adab7336c0e draft 'C'
  │ │
  │ o  1204d6864fcb draft 'D'
  │ │
  x │  27547f69f254 draft 'B'
  ├─╯
  o  4a2df7238c3b draft 'A'
Resume the rebasing:

  $ hg rebase --continue
  already rebased 27547f69f254 "B" as 8e6c056dc407
  rebasing 9adab7336c0e "C"
  merging A
  warning: 1 conflicts while merging A! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

Solve the conflict and go on:

  $ echo 'conflict solved' > A
  $ rm A.orig
  $ hg resolve -m A
  (no more unresolved files)
  continue: hg rebase --continue

  $ hg rebase --continue
  already rebased 27547f69f254 "B" as 8e6c056dc407
  rebasing 9adab7336c0e "C"

  $ tglogp
  o  bc920f6d31a8 draft 'C'
  │
  │ o  a1192b4e4efb draft 'Extra'
  │ │
  o │  8e6c056dc407 draft 'B'
  │ │
  @ │  a0c831b27e4d draft 'E'
  │ │
  │ x  9adab7336c0e draft 'C'
  │ │
  o │  1204d6864fcb draft 'D'
  │ │
  │ x  27547f69f254 draft 'B'
  ├─╯
  o  4a2df7238c3b draft 'A'
  $ cd ..


Changes during an interruption - abort:

  $ hg clone -q a a2
  $ cd a2
  $ hg pull -q -r $C -r $E
  $ hg go -q $E

  $ tglog
  @  a0c831b27e4d 'E'
  │
  │ o  9adab7336c0e 'C'
  │ │
  o │  1204d6864fcb 'D'
  │ │
  │ o  27547f69f254 'B'
  ├─╯
  o  4a2df7238c3b 'A'
Rebasing B onto E:

  $ hg rebase -s 'desc(B)' -d 'desc(E)'
  rebasing 27547f69f254 "B"
  rebasing 9adab7336c0e "C"
  merging A
  warning: 1 conflicts while merging A! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

Force a commit on B' during the interruption:

  $ save_off_rebasestate
  $ hg up -q -C 'max(desc(B))'
  $ restore_rebasestate

  $ echo 'Extra' > Extra
  $ hg add Extra
  $ save_off_rebasestate
  $ hg ci -m 'Extra'
  $ restore_rebasestate

  $ tglog
  @  c2ca60255d82 'Extra'
  │
  o  8e6c056dc407 'B'
  │
  o  a0c831b27e4d 'E'
  │
  │ o  9adab7336c0e 'C'
  │ │
  o │  1204d6864fcb 'D'
  │ │
  │ x  27547f69f254 'B'
  ├─╯
  o  4a2df7238c3b 'A'
Abort the rebasing:

  $ hg rebase --abort
  warning: new changesets detected on destination branch, can't strip
  rebase aborted

  $ tglog
  @  c2ca60255d82 'Extra'
  │
  o  8e6c056dc407 'B'
  │
  o  a0c831b27e4d 'E'
  │
  │ o  9adab7336c0e 'C'
  │ │
  o │  1204d6864fcb 'D'
  │ │
  │ x  27547f69f254 'B'
  ├─╯
  o  4a2df7238c3b 'A'
  $ cd ..

Changes during an interruption - abort (again):

  $ hg clone -q a a3
  $ cd a3
  $ hg pull -q -r $C -r $E
  $ hg go -q $E

  $ tglogp
  @  a0c831b27e4d draft 'E'
  │
  │ o  9adab7336c0e draft 'C'
  │ │
  o │  1204d6864fcb draft 'D'
  │ │
  │ o  27547f69f254 draft 'B'
  ├─╯
  o  4a2df7238c3b draft 'A'
Rebasing B onto E:

  $ hg rebase -s 'desc(B)' -d 'desc(E)'
  rebasing 27547f69f254 "B"
  rebasing 9adab7336c0e "C"
  merging A
  warning: 1 conflicts while merging A! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

Change phase on B and B'

  $ save_off_rebasestate
  $ hg up -q -C 'max(desc(B))'
  $ restore_rebasestate
  $ hg debugmakepublic 'desc(B)'

  $ tglogp
  @  8e6c056dc407 public 'B'
  │
  o  a0c831b27e4d public 'E'
  │
  │ o  9adab7336c0e draft 'C'
  │ │
  o │  1204d6864fcb public 'D'
  │ │
  │ o  27547f69f254 public 'B'
  ├─╯
  o  4a2df7238c3b public 'A'
Abort the rebasing:

  $ hg rebase --abort
  warning: can't clean up public changesets 8e6c056dc407
  rebase aborted

  $ tglogp
  @  8e6c056dc407 public 'B'
  │
  o  a0c831b27e4d public 'E'
  │
  │ o  9adab7336c0e draft 'C'
  │ │
  o │  1204d6864fcb public 'D'
  │ │
  │ o  27547f69f254 public 'B'
  ├─╯
  o  4a2df7238c3b public 'A'
Test rebase interrupted by hooks

  $ hg up 'desc(C)'
  2 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ echo F > F
  $ hg add F
  $ hg ci -m F

  $ cd ..

(precommit version)

  $ cp -R a3 hook-precommit
  $ cd hook-precommit
  $ hg rebase --source 'desc(C)' --dest 'max(desc(B))' --tool internal:other --config 'hooks.precommit=hg status | grep "M A"'
  rebasing 9adab7336c0e "C"
  M A
  rebasing 096735ac862c "F"
  abort: precommit hook exited with status 1
  [255]
  $ tglogp
  @  0004de432eb5 draft 'C'
  │
  │ @  096735ac862c draft 'F'
  │ │
  o │  8e6c056dc407 public 'B'
  │ │
  o │  a0c831b27e4d public 'E'
  │ │
  │ x  9adab7336c0e draft 'C'
  │ │
  o │  1204d6864fcb public 'D'
  │ │
  │ o  27547f69f254 public 'B'
  ├─╯
  o  4a2df7238c3b public 'A'
  $ hg rebase --continue
  already rebased 9adab7336c0e "C" as 0004de432eb5
  rebasing 096735ac862c "F"
  $ tglogp
  @  6c021db6ecaa draft 'F'
  │
  o  0004de432eb5 draft 'C'
  │
  o  8e6c056dc407 public 'B'
  │
  o  a0c831b27e4d public 'E'
  │
  o  1204d6864fcb public 'D'
  │
  │ o  27547f69f254 public 'B'
  ├─╯
  o  4a2df7238c3b public 'A'
  $ cd ..

Make sure merge state is cleaned up after a no-op rebase merge (issue5494)
  $ hg init repo
  $ cd repo
  $ echo a > a
  $ hg commit -qAm base
  $ echo b >> a
  $ hg commit -qm b
  $ hg up '.^'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo c >> a
  $ hg commit -qm c
  $ hg rebase -s 'max(desc(b))' -d 'desc(c)' --noninteractive
  rebasing fdaca8533b86 "b"
  merging a
  warning: 1 conflicts while merging a! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ echo a > a
  $ echo c >> a
  $ hg resolve --mark a
  (no more unresolved files)
  continue: hg rebase --continue
  $ hg rebase --continue
  rebasing fdaca8533b86 "b"
  note: not rebasing fdaca8533b86, its destination (rebasing onto) commit already has all its changes
  $ hg resolve --list
  $ test -f .hg/merge
  [1]
