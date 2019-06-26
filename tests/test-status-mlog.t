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
  $ rm -rf a .hg/blackbox*
  $ touch a
  $ hg status
  M a
  $ hg blackbox | grep 'a:'
  *> M a: size changed (2 -> 0), os.stat size = 0 (glob)

  $ sleep 1
  $ rm -rf .hg/blackbox*
  $ echo 1 > a
  $ hg status
  $ hg blackbox | grep 'a:'
  *> L a: mtime changed (* -> *) (glob)
  *> C a: checked in filesystem (glob)
