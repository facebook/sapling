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
  $ echo x >> x
  $ hg commit -qAm x2
  $ cd ..

  $ hgcloneshallow ssh://user@dummy/master shallow -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)

# Test that repack cleans up the old files and creates new packs

  $ cd shallow
  $ find $CACHEDIR -type f
  $TESTTMP/hgcache/repos
  $TESTTMP/hgcache/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/aee31534993a501858fb6dd96a065671922e7d51

  $ hg repack

  $ find $CACHEDIR -type f
  $TESTTMP/hgcache/repos
  $TESTTMP/hgcache/master/packs/e4e3c3b58b4be3368caa2996ff34a8bc21e7b01d.histidx
  $TESTTMP/hgcache/master/packs/e4e3c3b58b4be3368caa2996ff34a8bc21e7b01d.histpack
  $TESTTMP/hgcache/master/packs/817d294043bd21a3de01f807721971abe45219ce.dataidx
  $TESTTMP/hgcache/master/packs/817d294043bd21a3de01f807721971abe45219ce.datapack

# Test that the data in the new packs is accessible
  $ hg cat -r . x
  x
  x
