#chg-compatible

  $ configure mutation
  $ enable undo remotenames
  $ setconfig extensions.extralog="$TESTDIR/extralog.py"
  $ setconfig experimental.narrow-heads=true ui.interactive=true

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
  
