  $ setconfig extensions.treemanifest=!

  $ . "$TESTDIR/library.sh"

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > server=True
  > serverexpiration=-1
  > EOF
  $ echo x > x
  $ hg commit -qAm x
  $ echo y > y
  $ hg commit -qAm y
  $ echo z > z
  $ hg commit -qAm z
  $ cd ..

  $ hgcloneshallow ssh://user@dummy/master shallow -q
  3 files fetched over 1 fetches - (3 misses, 0.00% hit ratio) over *s (glob)

# Compute keepset for 0th and 2nd commit, which implies that we do not process
# the 1st commit, therefore we diff 2nd manifest with the 0th manifest and
# populate the keepkeys from the diff
  $ cd shallow
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > pullprefetch=0+2
  > EOF
  $ hg debugkeepset

# Compute keepset for all commits, which implies that we only process deltas of
# manifests of commits 1 and 2 and therefore populate the keepkeys from deltas
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > pullprefetch=all()
  > EOF
  $ hg debugkeepset
