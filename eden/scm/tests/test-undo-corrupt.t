
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
  > from sapling import error, revlog, vfs
  > opener = vfs.vfs('.sl/undolog')
  > log = revlog.revlog(opener, 'index.i')
  > lastrev = len(log) - 1
  > assert log._inline and log.length(lastrev) > 0
  > log.revision(lastrev)
  > with open('.sl/undolog/index.i', 'rb+') as f:
  >     f.seek(-1, os.SEEK_END)
  >     byte = f.read(1)
  >     f.seek(-1, os.SEEK_CUR)
  >     f.write(bytes([byte[0] ^ 1]))
  > try:
  >     revlog.revlog(opener, 'index.i').revision(lastrev)
  > except error.RevlogError:
  >     pass
  > else:
  >     raise AssertionError('undo log was not corrupted')
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
