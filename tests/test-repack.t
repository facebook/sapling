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
  $TESTTMP/hgcache/master/packs/bc0153a5326a2f0bcae9f659ad3376c04350119f.histidx
  $TESTTMP/hgcache/master/packs/bc0153a5326a2f0bcae9f659ad3376c04350119f.histpack
  $TESTTMP/hgcache/master/packs/817d294043bd21a3de01f807721971abe45219ce.dataidx
  $TESTTMP/hgcache/master/packs/817d294043bd21a3de01f807721971abe45219ce.datapack

# Test that the packs are readonly
  $ ls -l $CACHEDIR/master/packs
  * (glob)
  -r--r--r--* 817d294043bd21a3de01f807721971abe45219ce.dataidx (glob)
  -r--r--r--* 817d294043bd21a3de01f807721971abe45219ce.datapack (glob)
  -r--r--r--* bc0153a5326a2f0bcae9f659ad3376c04350119f.histidx (glob)
  -r--r--r--* bc0153a5326a2f0bcae9f659ad3376c04350119f.histpack (glob)

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
  $TESTTMP/hgcache/master/packs/bc0153a5326a2f0bcae9f659ad3376c04350119f.histidx
  $TESTTMP/hgcache/master/packs/bc0153a5326a2f0bcae9f659ad3376c04350119f.histpack
  $TESTTMP/hgcache/master/packs/817d294043bd21a3de01f807721971abe45219ce.dataidx
  $TESTTMP/hgcache/master/packs/817d294043bd21a3de01f807721971abe45219ce.datapack
  $TESTTMP/hgcache/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/d4a3ed9310e5bd9887e3bf779da5077efab28216

  $ hg repack --traceback

  $ find $CACHEDIR -type f
  $TESTTMP/hgcache/repos
  $TESTTMP/hgcache/master/packs/3ed57673383638cd7c2c873a5a00a1f40f26b0b8.histidx
  $TESTTMP/hgcache/master/packs/3ed57673383638cd7c2c873a5a00a1f40f26b0b8.histpack
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
  $TESTTMP/hgcache/master/packs/3ed57673383638cd7c2c873a5a00a1f40f26b0b8.histidx
  $TESTTMP/hgcache/master/packs/3ed57673383638cd7c2c873a5a00a1f40f26b0b8.histpack
  $TESTTMP/hgcache/master/packs/1e386660a2bca1c6949a1cbf5b095765e98fd241.dataidx
  $TESTTMP/hgcache/master/packs/1e386660a2bca1c6949a1cbf5b095765e98fd241.datapack

# Run two repacks at once
  $ hg repack --config "hooks.prerepack=sleep 2" &
  $ hg repack
  abort: skipping repack - another repack is already running
  [255]
  $ sleep 3

# Run repack in the background
  $ cd ../master
  $ echo x >> x
  $ hg commit -m x4
  $ cd ../shallow
  $ hg pull -q
  $ hg up -q tip
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ find $CACHEDIR -type f
  $TESTTMP/hgcache/repos
  $TESTTMP/hgcache/master/packs/3ed57673383638cd7c2c873a5a00a1f40f26b0b8.histidx
  $TESTTMP/hgcache/master/packs/3ed57673383638cd7c2c873a5a00a1f40f26b0b8.histpack
  $TESTTMP/hgcache/master/packs/1e386660a2bca1c6949a1cbf5b095765e98fd241.dataidx
  $TESTTMP/hgcache/master/packs/1e386660a2bca1c6949a1cbf5b095765e98fd241.datapack
  $TESTTMP/hgcache/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/1bb2e6237e035c8f8ef508e281f1ce075bc6db72
  $ hg repack --background
  $ sleep 2
  $ find $CACHEDIR -type f
  $TESTTMP/hgcache/repos
  $TESTTMP/hgcache/master/packs/3bebfba849e7aed8e598b92b296aeaff4784393b.histidx
  $TESTTMP/hgcache/master/packs/3bebfba849e7aed8e598b92b296aeaff4784393b.histpack
  $TESTTMP/hgcache/master/packs/92a06d8b76a23b6e6150cf877ea75ed993e0b2d8.dataidx
  $TESTTMP/hgcache/master/packs/92a06d8b76a23b6e6150cf877ea75ed993e0b2d8.datapack

# Test copy tracing from a pack
  $ cd ../master
  $ hg mv x y
  $ hg commit -m 'move x to y'
  $ cd ../shallow
  $ hg pull -q
  $ hg up -q tip
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ hg repack
  $ hg log -f y -T '{desc}\n'
  move x to y
  x4
  x3
  x2
  x
