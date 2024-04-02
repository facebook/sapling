#debugruntest-compatible

#require no-eden


  $ eagerepo
  $ enable journal undo rebase

  $ newrepo
  $ drawdag <<'EOS'
  > G K
  > :/
  > E
  > |\
  > C D
  > |/
  > B
  > |
  > A
  > EOS

  $ hg go $A -q
  $ hg go $D -q

  $ hg go -
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg log -r . -T '{desc}\n'
  A

  $ hg go -
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -r . -T '{desc}\n'
  D

  $ hg go $C -q
  $ hg go -
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg log -r . -T '{desc}\n'
  D

test merge commit

  $ hg go $E -q
  $ hg go -
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg log -r . -T '{desc}\n'
  D

  $ hg go -
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -r . -T '{desc}\n'
  E

test undo commands

  $ echo 1 >> x
  $ hg ci -Aqm X
  $ hg log -r . -T '{desc}\n'
  X
  $ hg undo -q
  $ hg log -r . -T '{desc}\n'
  E
  $ hg go -
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -r . -T '{desc}\n'
  X

test amend command
  $ hg go $D -q
  $ hg go $K -q
  $ echo 1 >> K
  $ hg amend
  $ hg go -
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg log -r . -T '{desc}\n'
  D

test amend & restack commands

  $ hg go $D -q
  $ hg go $F -q
  $ echo 1 >> F
  $ hg amend
  hint[amend-restack]: descendants of 8059b7e18560 are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ hg rebase --restack
  rebasing bffd6b0484a3 "G"
  $ hg go -
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg log -r . -T '{desc}\n'
  D
