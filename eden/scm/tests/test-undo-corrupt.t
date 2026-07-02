
  $ configure modern
  $ enable undo
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
  > with open('.sl/undolog/index.i', 'rb+') as f:
  >     f.seek(-10, os.SEEK_END)
  >     f.write(b"x")
  > EOF

  $ sl debugpython a.py

Command should not abort

  $ sl debugdrawdag << 'EOS'
  >   C
  >   |
  > desc(A)
  > EOS
  hint[undo-corrupt]: undo history is corrupted
  (try deleting $TESTTMP/repo1/.sl/undolog to recover)

Undo itself does not crash

  $ sl undo
  cannot undo: undo history is corrupted
  hint[undo-corrupt]: undo history is corrupted
  (try deleting $TESTTMP/repo1/.sl/undolog to recover)
