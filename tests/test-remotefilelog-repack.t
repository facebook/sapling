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
  $ find $CACHEDIR | sort
  $TESTTMP/hgcache
  $TESTTMP/hgcache/master
  $TESTTMP/hgcache/master/11
  $TESTTMP/hgcache/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072
  $TESTTMP/hgcache/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/aee31534993a501858fb6dd96a065671922e7d51
  $TESTTMP/hgcache/repos

  $ hg repack

  $ find $CACHEDIR | sort
  $TESTTMP/hgcache
  $TESTTMP/hgcache/master
  $TESTTMP/hgcache/master/packs
  $TESTTMP/hgcache/master/packs/817d294043bd21a3de01f807721971abe45219ce.dataidx
  $TESTTMP/hgcache/master/packs/817d294043bd21a3de01f807721971abe45219ce.datapack
  $TESTTMP/hgcache/master/packs/bc0153a5326a2f0bcae9f659ad3376c04350119f.histidx
  $TESTTMP/hgcache/master/packs/bc0153a5326a2f0bcae9f659ad3376c04350119f.histpack
  $TESTTMP/hgcache/repos

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

  $ find $CACHEDIR -type f | sort
  $TESTTMP/hgcache/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/d4a3ed9310e5bd9887e3bf779da5077efab28216
  $TESTTMP/hgcache/master/packs/817d294043bd21a3de01f807721971abe45219ce.dataidx
  $TESTTMP/hgcache/master/packs/817d294043bd21a3de01f807721971abe45219ce.datapack
  $TESTTMP/hgcache/master/packs/bc0153a5326a2f0bcae9f659ad3376c04350119f.histidx
  $TESTTMP/hgcache/master/packs/bc0153a5326a2f0bcae9f659ad3376c04350119f.histpack
  $TESTTMP/hgcache/repos

  $ hg repack --traceback

  $ find $CACHEDIR -type f | sort
  $TESTTMP/hgcache/master/packs/1e386660a2bca1c6949a1cbf5b095765e98fd241.dataidx
  $TESTTMP/hgcache/master/packs/1e386660a2bca1c6949a1cbf5b095765e98fd241.datapack
  $TESTTMP/hgcache/master/packs/3ed57673383638cd7c2c873a5a00a1f40f26b0b8.histidx
  $TESTTMP/hgcache/master/packs/3ed57673383638cd7c2c873a5a00a1f40f26b0b8.histpack
  $TESTTMP/hgcache/repos

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
  $ find $CACHEDIR -type f | sort
  $TESTTMP/hgcache/master/packs/1e386660a2bca1c6949a1cbf5b095765e98fd241.dataidx
  $TESTTMP/hgcache/master/packs/1e386660a2bca1c6949a1cbf5b095765e98fd241.datapack
  $TESTTMP/hgcache/master/packs/3ed57673383638cd7c2c873a5a00a1f40f26b0b8.histidx
  $TESTTMP/hgcache/master/packs/3ed57673383638cd7c2c873a5a00a1f40f26b0b8.histpack
  $TESTTMP/hgcache/repos

# Run two repacks at once
  $ hg repack --config "hooks.prerepack=sleep 3" &
  $ sleep 1
  $ hg repack
  abort: skipping repack - another repack is already running
  [255]
  $ hg debugwaitonrepack >/dev/null 2>&1

# Run repack in the background
  $ cd ../master
  $ echo x >> x
  $ hg commit -m x4
  $ cd ../shallow
  $ hg pull -q
  $ hg up -q tip
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ find $CACHEDIR -type f | sort
  $TESTTMP/hgcache/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/1bb2e6237e035c8f8ef508e281f1ce075bc6db72
  $TESTTMP/hgcache/master/packs/1e386660a2bca1c6949a1cbf5b095765e98fd241.dataidx
  $TESTTMP/hgcache/master/packs/1e386660a2bca1c6949a1cbf5b095765e98fd241.datapack
  $TESTTMP/hgcache/master/packs/3ed57673383638cd7c2c873a5a00a1f40f26b0b8.histidx
  $TESTTMP/hgcache/master/packs/3ed57673383638cd7c2c873a5a00a1f40f26b0b8.histpack
  $TESTTMP/hgcache/repos
  $ hg repack --background
  (running background repack)
  $ sleep 0.5
  $ hg debugwaitonrepack >/dev/null 2>&1
  $ find $CACHEDIR -type f | sort
  $TESTTMP/hgcache/master/packs/3bebfba849e7aed8e598b92b296aeaff4784393b.histidx
  $TESTTMP/hgcache/master/packs/3bebfba849e7aed8e598b92b296aeaff4784393b.histpack
  $TESTTMP/hgcache/master/packs/92a06d8b76a23b6e6150cf877ea75ed993e0b2d8.dataidx
  $TESTTMP/hgcache/master/packs/92a06d8b76a23b6e6150cf877ea75ed993e0b2d8.datapack
  $TESTTMP/hgcache/repos

# Test debug commands

  $ hg debugdatapack $TESTTMP/hgcache/master/packs/92a06d8b76a23b6e6150cf877ea75ed993e0b2d8
  
  x
  Node          Delta Base    Delta Length
  1bb2e6237e03  000000000000  8
  d4a3ed9310e5  1bb2e6237e03  12
  aee31534993a  d4a3ed9310e5  12
  $ hg debugdatapack --long $TESTTMP/hgcache/master/packs/92a06d8b76a23b6e6150cf877ea75ed993e0b2d8
  
  x
  Node                                      Delta Base                                Delta Length
  1bb2e6237e035c8f8ef508e281f1ce075bc6db72  0000000000000000000000000000000000000000  8
  d4a3ed9310e5bd9887e3bf779da5077efab28216  1bb2e6237e035c8f8ef508e281f1ce075bc6db72  12
  aee31534993a501858fb6dd96a065671922e7d51  d4a3ed9310e5bd9887e3bf779da5077efab28216  12
  $ hg debugdatapack $TESTTMP/hgcache/master/packs/92a06d8b76a23b6e6150cf877ea75ed993e0b2d8 --node d4a3ed9310e5bd9887e3bf779da5077efab28216
  
  x
  Node                                      Delta Base                                Delta SHA1                                Delta Length
  d4a3ed9310e5bd9887e3bf779da5077efab28216  1bb2e6237e035c8f8ef508e281f1ce075bc6db72  77029ab56e83ea2115dd53ff87483682abe5d7ca  12
  Node                                      Delta Base                                Delta SHA1                                Delta Length
  1bb2e6237e035c8f8ef508e281f1ce075bc6db72  0000000000000000000000000000000000000000  7ca8c71a64f7b56380e77573da2f7a5fdd2ecdb5  8
  $ hg debughistorypack $TESTTMP/hgcache/master/packs/3bebfba849e7aed8e598b92b296aeaff4784393b
  
  x
  Node          P1 Node       P2 Node       Link Node     Copy From
  1bb2e6237e03  d4a3ed9310e5  000000000000  0b03bbc9e1e7  
  d4a3ed9310e5  aee31534993a  000000000000  421535db10b6  
  aee31534993a  1406e7411862  000000000000  a89d614e2364  
  1406e7411862  000000000000  000000000000  b292c1e3311f  

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

# Test copy trace across rename and back
  $ cp -R $TESTTMP/hgcache/master/packs $TESTTMP/backuppacks
  $ cd ../master
  $ hg mv y x
  $ hg commit -m 'move y back to x'
  $ hg revert -r 0 x
  $ mv x y
  $ hg add y
  $ hg revert x
  $ hg commit -m 'add y back without metadata'
  $ cd ../shallow
  $ hg pull -q
  $ hg up -q tip
  2 files fetched over 2 fetches - (2 misses, 0.00% hit ratio) over * (glob)
  $ hg repack
  $ ls $TESTTMP/hgcache/master/packs
  2c833052b8b6b9d310b424403e87997bc7735459.dataidx
  2c833052b8b6b9d310b424403e87997bc7735459.datapack
  b6be6cc48737aa69dc05ec02575536846c67a471.histidx
  b6be6cc48737aa69dc05ec02575536846c67a471.histpack
  $ hg debughistorypack $TESTTMP/hgcache/master/packs/b6be6cc48737aa69dc05ec02575536846c67a471
  
  x
  Node          P1 Node       P2 Node       Link Node     Copy From
  cd410a44d584  577959738234  000000000000  609547eda446  y
  1bb2e6237e03  d4a3ed9310e5  000000000000  0b03bbc9e1e7  
  d4a3ed9310e5  aee31534993a  000000000000  421535db10b6  
  aee31534993a  1406e7411862  000000000000  a89d614e2364  
  1406e7411862  000000000000  000000000000  b292c1e3311f  
  
  y
  Node          P1 Node       P2 Node       Link Node     Copy From
  577959738234  1bb2e6237e03  000000000000  c7faf2fc439a  x
  1406e7411862  000000000000  000000000000  b292c1e3311f  
  $ hg strip -r '.^'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/shallow/.hg/strip-backup/609547eda446-1aa878d4-backup.hg (glob)
  $ hg -R ../master strip -r '.^'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/master/.hg/strip-backup/609547eda446-1aa878d4-backup.hg (glob)

  $ rm -rf $TESTTMP/hgcache/master/packs
  $ cp -R $TESTTMP/backuppacks $TESTTMP/hgcache/master/packs

# Test repacking datapack without history
  $ rm -rf $CACHEDIR/master/packs/*hist*
  $ hg repack
  $ hg debugdatapack $TESTTMP/hgcache/master/packs/1c6261363473d5595d26728c201e1395d39bf94e
  
  x
  Node          Delta Base    Delta Length
  1bb2e6237e03  000000000000  8
  aee31534993a  d4a3ed9310e5  12
  d4a3ed9310e5  1bb2e6237e03  12
  
  y
  Node          Delta Base    Delta Length
  577959738234  000000000000  70

  $ hg cat -r ".^" x
  x
  x
  x
  x

Incremental repack
  $ rm -rf $CACHEDIR/master/packs/*
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > data.generations=60
  >   150
  > fetchpacks=True
  > EOF

Single pack - repack does nothing
  $ hg prefetch -r 0
  1 files fetched over 1 fetches - (0 misses, 100.00% hit ratio) over * (glob)
  $ ls -l $TESTTMP/hgcache/master/packs/ | grep datapack
  * 59 * 5b7dec902026f0cddb0ef8acb62f27b5698494d4.datapack (glob)
  $ ls -l $TESTTMP/hgcache/master/packs/ | grep histpack
  * 90 * 29e5896bfbe67389c4f98b18c18d54706b80fafb.histpack (glob)
  $ hg repack --incremental
  $ ls -l $TESTTMP/hgcache/master/packs/ | grep datapack
  * 59 * 5b7dec902026f0cddb0ef8acb62f27b5698494d4.datapack (glob)
  $ ls -l $TESTTMP/hgcache/master/packs/ | grep histpack
  * 90 * 29e5896bfbe67389c4f98b18c18d54706b80fafb.histpack (glob)

3 gen1 packs, 1 gen0 pack - packs 3 gen1 into 1
  $ hg prefetch -r 1
  1 files fetched over 1 fetches - (0 misses, 100.00% hit ratio) over * (glob)
  $ hg prefetch -r 2
  1 files fetched over 1 fetches - (0 misses, 100.00% hit ratio) over * (glob)
  $ hg prefetch -r 3
  1 files fetched over 1 fetches - (0 misses, 100.00% hit ratio) over * (glob)
  $ ls -l $TESTTMP/hgcache/master/packs/ | grep datapack
  * 59 * 5b7dec902026f0cddb0ef8acb62f27b5698494d4.datapack (glob)
  * 65 * 6c499d21350d79f92fd556b4b7a902569d88e3c9.datapack (glob)
  * 61 * 817d294043bd21a3de01f807721971abe45219ce.datapack (glob)
  * 63 * ff45add45ab3f59c4f75efc6a087d86c821219d6.datapack (glob)
  $ ls -l $TESTTMP/hgcache/master/packs/ | grep histpack
  *  90 * 29e5896bfbe67389c4f98b18c18d54706b80fafb.histpack (glob)
  * 336 * 3bebfba849e7aed8e598b92b296aeaff4784393b.histpack (glob)
  * 254 * 3ed57673383638cd7c2c873a5a00a1f40f26b0b8.histpack (glob)
  * 172 * bc0153a5326a2f0bcae9f659ad3376c04350119f.histpack (glob)
  $ hg repack --incremental
  $ ls -l $TESTTMP/hgcache/master/packs/ | grep datapack
  *  59 * 5b7dec902026f0cddb0ef8acb62f27b5698494d4.datapack (glob)
  * 201 * 92a06d8b76a23b6e6150cf877ea75ed993e0b2d8.datapack (glob)
  $ ls -l $TESTTMP/hgcache/master/packs/ | grep histpack
  * 336 * 3bebfba849e7aed8e598b92b296aeaff4784393b.histpack (glob)

1 gen3 pack, 1 gen0 pack - does nothing
  $ hg repack --incremental
  $ ls -l $TESTTMP/hgcache/master/packs/ | grep datapack
  *  59 * 5b7dec902026f0cddb0ef8acb62f27b5698494d4.datapack (glob)
  * 201 * 92a06d8b76a23b6e6150cf877ea75ed993e0b2d8.datapack (glob)
  $ ls -l $TESTTMP/hgcache/master/packs/ | grep histpack
  * 336 * 3bebfba849e7aed8e598b92b296aeaff4784393b.histpack (glob)

Pull should run background repack
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > backgroundrepack=True
  > EOF
  $ clearcache
  $ hg prefetch -r 0
  1 files fetched over 1 fetches - (0 misses, 100.00% hit ratio) over * (glob)
  $ hg prefetch -r 1
  1 files fetched over 1 fetches - (0 misses, 100.00% hit ratio) over * (glob)
  $ hg prefetch -r 2
  1 files fetched over 1 fetches - (0 misses, 100.00% hit ratio) over * (glob)
  $ hg prefetch -r 3
  1 files fetched over 1 fetches - (0 misses, 100.00% hit ratio) over * (glob)
  $ ls -l $TESTTMP/hgcache/master/packs/ | grep datapack
  * 59 * 5b7dec902026f0cddb0ef8acb62f27b5698494d4.datapack (glob)
  * 65 * 6c499d21350d79f92fd556b4b7a902569d88e3c9.datapack (glob)
  * 61 * 817d294043bd21a3de01f807721971abe45219ce.datapack (glob)
  * 63 * ff45add45ab3f59c4f75efc6a087d86c821219d6.datapack (glob)
  $ ls -l $TESTTMP/hgcache/master/packs/ | grep histpack
  *  90 * 29e5896bfbe67389c4f98b18c18d54706b80fafb.histpack (glob)
  * 336 * 3bebfba849e7aed8e598b92b296aeaff4784393b.histpack (glob)
  * 254 * 3ed57673383638cd7c2c873a5a00a1f40f26b0b8.histpack (glob)
  * 172 * bc0153a5326a2f0bcae9f659ad3376c04350119f.histpack (glob)

  $ hg pull
  pulling from ssh://user@dummy/master
  searching for changes
  no changes found
  (running background incremental repack)
  $ sleep 0.5
  $ hg debugwaitonrepack >/dev/null 2>&1
  $ ls -l $TESTTMP/hgcache/master/packs/ | grep datapack
  *  59 * 5b7dec902026f0cddb0ef8acb62f27b5698494d4.datapack (glob)
  * 201 * 92a06d8b76a23b6e6150cf877ea75ed993e0b2d8.datapack (glob)
  $ ls -l $TESTTMP/hgcache/master/packs/ | grep histpack
  * 336 * 3bebfba849e7aed8e598b92b296aeaff4784393b.histpack (glob)

Test environment variable resolution
  $ CACHEPATH=$TESTTMP/envcache hg prefetch --config 'remotefilelog.cachepath=$CACHEPATH'
  1 files fetched over 1 fetches - (0 misses, 100.00% hit ratio) over * (glob)
  $ find $TESTTMP/envcache | sort
  $TESTTMP/envcache
  $TESTTMP/envcache/master
  $TESTTMP/envcache/master/packs
  $TESTTMP/envcache/master/packs/54afbfda203716c1aa2636029ccc0df18165129e.dataidx
  $TESTTMP/envcache/master/packs/54afbfda203716c1aa2636029ccc0df18165129e.datapack
  $TESTTMP/envcache/master/packs/f842b0cc2f54b1b5719584b449afdf58b08ab006.histidx
  $TESTTMP/envcache/master/packs/f842b0cc2f54b1b5719584b449afdf58b08ab006.histpack

Test local remotefilelog blob is correct when based on a pack
  $ hg prefetch -r .
  1 files fetched over 1 fetches - (0 misses, 100.00% hit ratio) over * (glob)
  $ echo >> y
  $ hg commit -m y2
  $ hg debugremotefilelog .hg/store/data/95cb0bfd2977c761298d9624e4b4d4c72a39974a/b70860edba4f8242a1d52f2a94679dd23cb76808
  size: 9 bytes
  path: .hg/store/data/95cb0bfd2977c761298d9624e4b4d4c72a39974a/b70860edba4f8242a1d52f2a94679dd23cb76808 
  key: b70860edba4f 
  
          node =>           p1            p2      linknode     copyfrom
  b70860edba4f => 577959738234  000000000000  08d3fbc98c48  
  577959738234 => 1bb2e6237e03  000000000000  c7faf2fc439a  x
  1bb2e6237e03 => d4a3ed9310e5  000000000000  0b03bbc9e1e7  
  d4a3ed9310e5 => aee31534993a  000000000000  421535db10b6  
  aee31534993a => 1406e7411862  000000000000  a89d614e2364  
  1406e7411862 => 000000000000  000000000000  b292c1e3311f  
