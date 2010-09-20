  $ echo "[extensions]" >> $HGRCPATH
  $ echo "mq=" >> $HGRCPATH

  $ hg init

  $ echo 'base' > base
  $ hg ci -Ambase
  adding base

  $ hg qnew -mmqbase mqbase

  $ hg qsave
  $ hg qrestore 2
  restoring status: hg patches saved state

