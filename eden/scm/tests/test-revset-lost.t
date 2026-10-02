
#require no-eden

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
  $ sl log -r 'lost()'

Hiding a commit also hides its descendants:
  $ sl hide $B -q
  $ sl log -r 'lost()' -T '{desc}\n'
  B
  C
  E
  D

`unhide` makes a commit and its ancestors no longer lost:
  $ sl unhide $D
  $ sl log -r 'lost()' -T '{desc}\n'
  E
  $ sl unhide $E
  $ sl log -r 'lost()'

`drop` in `histedit` can produce lost commits:
  $ sl up $D -q
  $ sl histedit $C --commands - <<EOF
  > pick $D
  > drop $C
  > EOF
  dropping changeset 26805a: C
  $ sl log -r 'lost()' -T '{desc}\n'
  C

`amend` (or `metaedit`) does not make commits lost if they have successors:
  $ newrepo
  $ sl commit -m A -q
  $ sl amend -m B
  $ sl amend -m C
  $ sl amend -m D
  $ sl log -r 'lost()' # Nothing is lost initially
  $ sl hide '.' -q
  $ sl log -r 'lost()' -T '{desc}\n'
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
  $ sl log -r 'lost()' # Nothing is lost initially
  $ sl hide $C -q
  $ sl hide $B -q
  $ sl hide $E -q
  $ sl hide $D -q
  $ sl log -r 'lost()' -T '{desc}\n'
  D
  E
  B
  C
