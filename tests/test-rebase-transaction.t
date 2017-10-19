  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > rebase=
  > drawdag=$TESTDIR/drawdag.py
  > 
  > [phases]
  > publish=False
  > 
  > [alias]
  > tglog = log -G --template "{rev}: {desc}"
  > EOF

Rebasing using a single transaction

  $ hg init singletr && cd singletr
  $ cat >> .hg/hgrc <<EOF
  > [rebase]
  > singletransaction=True
  > EOF
  $ hg debugdrawdag <<'EOF'
  >   Z
  >   |
  >   | D
  >   | |
  >   | C
  >   | |
  >   Y B
  >   |/
  >   A
  > EOF
- We should only see two status stored messages. One from the start, one from
- the end.
  $ hg rebase --debug -b D -d Z | grep 'status stored'
  rebase status stored
  rebase status stored
  $ hg tglog
  o  5: D
  |
  o  4: C
  |
  o  3: B
  |
  o  2: Z
  |
  o  1: Y
  |
  o  0: A
  
  $ cd ..
