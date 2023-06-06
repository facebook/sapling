#chg-compatible

  $ configure modernclient
  $ newclientrepo repo
  $ enable undo

  $ drawdag <<'EOS'
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
