#chg-compatible

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > undo=
  > remotenames=
  > extralog=$TESTDIR/extralog.py
  > [experimental]
  > evolution=
  > narrow-heads=true
  > [visibility]
  > enabled=true
  > [mutation]
  > enabled=true
  > date=0 0
  > [ui]
  > interactive = true
  > EOF

  $ newrepo
  $ drawdag << 'EOS'
  > B
  > |
  > A
  > EOS

  $ drawdag << 'EOS'
  >   C
  >  /
  > A
  > EOS

  $ hg undo
  undone to *, before book -fd A C (glob)
  $ hg undo
  undone to *, before debugdrawdag (glob)
  $ hg log -GT '{desc}'
  o  B
  |
  o  A
  
  $ hg redo
  undone to *, before undo (glob)
  $ hg log -GT '{desc}'
  o  C
  |
  | o  B
  |/
  o  A
  
