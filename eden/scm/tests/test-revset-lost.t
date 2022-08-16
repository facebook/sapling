#chg-compatible
#debugruntest-compatible
  $ configure modern
  $ setconfig ui.allowemptycommit=1
  $ enable histedit

Configure repo:
  $ newrepo
  $ drawdag << 'EOS'
  > D
  > |
  > C E
  > |/
  > B
  > |
  > A
  > EOS

Nothing is lost initially:
  $ hg log -r 'lost()'

Hiding a commit also hides its descendants:
  $ hg hide $B -q
  $ hg log -r 'lost()' -T '{desc}\n'
  B
  C
  E
  D

`unhide` makes a commit and its ancestors no longer lost:
  $ hg unhide $D
  $ hg log -r 'lost()' -T '{desc}\n'
  E
  $ hg unhide $E
  $ hg log -r 'lost()'

`drop` in `histedit` can produce lost commits:
  $ hg up $D -q
  $ hg histedit $C --commands - <<EOF
  > pick $D
  > drop $C
  > EOF
  $ hg log -r 'lost()' -T '{desc}\n'
  C

`amend` (or `metaedit`) does not make commits lost if they have successors:
  $ newrepo
  $ hg commit -m A -q
  $ hg amend -m B
  $ hg amend -m C
  $ hg amend -m D
  $ hg log -r 'lost()' # Nothing is lost initially
  $ hg hide '.' -q
  $ hg log -r 'lost()' -T '{desc}\n'
  D

Lost nodes are sorted by most recent hidden first:
  $ newrepo
  $ drawdag << 'EOS'
  > E
  > | D
  > |/C
  > |/B
  > |/
  > A
  > EOS
  $ hg log -r 'lost()' # Nothing is lost initially
  $ hg hide $C -q
  $ hg hide $B -q
  $ hg hide $E -q
  $ hg hide $D -q
  $ hg log -r 'lost()' -T '{desc}\n'
  D
  E
  B
  C
