#debugruntest-compatible
  $ configure modern
  $ newrepo
  $ drawdag << 'EOS'
  > A
  > EOS

  $ hg debuginternals
  *	blackbox (glob)
  *	store/manifests (glob)
  *	store/metalog (glob)
  *	store/mutation (glob)

  $ hg debuginternals -o a.tar.gz 2>/dev/null
  a.tar.gz
