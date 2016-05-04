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

# Test that the packs are readonly
  $ ls -l $CACHEDIR/master/packs
  * (glob)
  -r--r--r--* 817d294043bd21a3de01f807721971abe45219ce.dataidx (glob)
  -r--r--r--* 817d294043bd21a3de01f807721971abe45219ce.datapack (glob)
  -r--r--r--* e4e3c3b58b4be3368caa2996ff34a8bc21e7b01d.histidx (glob)
  -r--r--r--* e4e3c3b58b4be3368caa2996ff34a8bc21e7b01d.histpack (glob)

# Test that the data in the new packs is accessible
  $ hg cat -r . x
  x
  x

# Test that adding new data and repacking it results in the loose data and the
# old packs being combined.

  $ cd ../master
  $ echo x >> x
  $ hg commit -m x3
  $ cd ../shallow
  $ hg pull -q
  $ hg up -q tip
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)

  $ find $CACHEDIR -type f
  $TESTTMP/hgcache/repos
  $TESTTMP/hgcache/master/packs/e4e3c3b58b4be3368caa2996ff34a8bc21e7b01d.histidx
  $TESTTMP/hgcache/master/packs/e4e3c3b58b4be3368caa2996ff34a8bc21e7b01d.histpack
  $TESTTMP/hgcache/master/packs/817d294043bd21a3de01f807721971abe45219ce.dataidx
  $TESTTMP/hgcache/master/packs/817d294043bd21a3de01f807721971abe45219ce.datapack
  $TESTTMP/hgcache/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/d4a3ed9310e5bd9887e3bf779da5077efab28216

  $ hg repack --traceback

  $ find $CACHEDIR -type f
  $TESTTMP/hgcache/repos
  $TESTTMP/hgcache/master/packs/cf96d8e28f1cf03420d033984ac3f90d6711b7dd.histidx
  $TESTTMP/hgcache/master/packs/cf96d8e28f1cf03420d033984ac3f90d6711b7dd.histpack
  $TESTTMP/hgcache/master/packs/1e386660a2bca1c6949a1cbf5b095765e98fd241.dataidx
  $TESTTMP/hgcache/master/packs/1e386660a2bca1c6949a1cbf5b095765e98fd241.datapack

# Verify all the file data is still available
  $ hg cat -r . x
  x
  x
  x
  $ hg cat -r '.^' x
  x
  x

# Test that repacking again without new data does not delete the pack files
# and did not change the pack names
  $ hg repack
  $ find $CACHEDIR -type f
  $TESTTMP/hgcache/repos
  $TESTTMP/hgcache/master/packs/cf96d8e28f1cf03420d033984ac3f90d6711b7dd.histidx
  $TESTTMP/hgcache/master/packs/cf96d8e28f1cf03420d033984ac3f90d6711b7dd.histpack
  $TESTTMP/hgcache/master/packs/1e386660a2bca1c6949a1cbf5b095765e98fd241.dataidx
  $TESTTMP/hgcache/master/packs/1e386660a2bca1c6949a1cbf5b095765e98fd241.datapack
