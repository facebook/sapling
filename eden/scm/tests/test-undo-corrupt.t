#chg-compatible
#debugruntest-incompatible

  $ configure modern
  $ enable undo remotenames
  $ setconfig hint.ack-hint-ack=1

  $ newrepo
  $ drawdag << 'EOS'
  > B
  > |
  > A
  > EOS

Break the undo log

  $ cat > a.py << EOF
  > import os
  > with open('.hg/undolog/index.i', 'rb+') as f:
  >     f.seek(-10, os.SEEK_END)
  >     f.write(b"x")
  > EOF

  $ hg debugpython a.py

Command should not abort

  $ hg debugdrawdag << 'EOS'
  >   C
  >   |
  > desc(A)
  > EOS
  hint[undo-corrupt]: undo history is corrupted
  (try deleting $TESTTMP/repo1/.hg/undolog to recover)

Undo itself does not crash

  $ hg undo
  cannot undo: undo history is corrupted
  hint[undo-corrupt]: undo history is corrupted
  (try deleting $TESTTMP/repo1/.hg/undolog to recover)

