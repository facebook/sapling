#debugruntest-compatible
  $ setconfig format.use-segmented-changelog=true
  $ setconfig devel.segmented-changelog-rev-compat=true
  $ configure modern
  $ newrepo
  $ drawdag << 'EOS'
  > A
  > EOS

  $ hg debuginternals
  *	blackbox (glob)
  *	store/hgcommits (glob)
  *	store/manifests (glob)
  *	store/metalog (glob)
  *	store/mutation (glob)
  *	store/segments (glob)

  $ hg debuginternals -o a.tar.gz 2>/dev/null
  a.tar.gz
