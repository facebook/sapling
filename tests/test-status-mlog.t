Test logging of "M" entries

  $ newrepo
  $ enable blackbox
  $ setconfig experimental.samplestatus=2 blackbox.track=status

  $ echo 1 > a
  $ hg commit -A a -m a

  $ echo 2 >> a
  $ hg status
  M a
  $ hg blackbox | grep 'a:'
  *> M a: size changed (2 -> 4) (glob)

  $ sleep 1
  $ rm .hg/blackbox*
  $ echo 3 > a
  $ hg status
  M a
  $ hg blackbox | grep 'a:'
  *> L a: mtime changed (* -> *) (glob)
  *> M a: checked in filesystem (glob)

  $ sleep 1
  $ rm .hg/blackbox*
  $ echo 1 > a
  $ hg status
  $ hg blackbox | grep 'a:'
  *> L a: mtime changed (* -> *) (glob)
  *> C a: checked in filesystem (glob)
