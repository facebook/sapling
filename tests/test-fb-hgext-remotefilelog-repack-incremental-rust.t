  $ setconfig extensions.treemanifest=!

  $ . "$TESTDIR/library.sh"

  $ cat >> $HGRCPATH <<EOF
  > [remotefilelog]
  > fetchpacks=True
  > EOF

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
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)

# Set the prefetchdays config to zero so that all commits are prefetched
# no matter what their creation date is.
  $ cd shallow
  $ setconfig remotefilelog.prefetchdays=0 remotefilelog.userustrepack=True
  $ cd ..

# Test that repack cleans up the old files and creates new packs

  $ cd shallow
  $ find $CACHEDIR | sort
  $TESTTMP/hgcache
  $TESTTMP/hgcache/master
  $TESTTMP/hgcache/master/packs
  $TESTTMP/hgcache/master/packs/276d308429d0303762befa376788300f0310f90e.histidx
  $TESTTMP/hgcache/master/packs/276d308429d0303762befa376788300f0310f90e.histpack
  $TESTTMP/hgcache/master/packs/887690f1138ae5b99c50d754ed02262874bf8ecb.dataidx
  $TESTTMP/hgcache/master/packs/887690f1138ae5b99c50d754ed02262874bf8ecb.datapack

  $ cd ../master
  $ echo x3 > x
  $ hg commit -qAm x3
  $ echo x4 > x
  $ hg commit -qAm x4
  $ cd ../shallow
  $ hg pull -q
  $ hg up -q tip
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)

3 gen1 packs, 1 gen0 pack - packs 3 gen1 into 1
  $ hg prefetch -r 0
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ hg prefetch -r 2
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ find $CACHEDIR | sort
  $TESTTMP/hgcache
  $TESTTMP/hgcache/master
  $TESTTMP/hgcache/master/packs
  $TESTTMP/hgcache/master/packs/1e6f0f575de6319f747ef83966a08775803fcecc.dataidx
  $TESTTMP/hgcache/master/packs/1e6f0f575de6319f747ef83966a08775803fcecc.datapack
  $TESTTMP/hgcache/master/packs/276d308429d0303762befa376788300f0310f90e.histidx
  $TESTTMP/hgcache/master/packs/276d308429d0303762befa376788300f0310f90e.histpack
  $TESTTMP/hgcache/master/packs/2d66e09c3bf8a000428af1630d978127182e496e.dataidx
  $TESTTMP/hgcache/master/packs/2d66e09c3bf8a000428af1630d978127182e496e.datapack
  $TESTTMP/hgcache/master/packs/3266aa7480df06153adccad2f1abb6d11f42de0e.dataidx
  $TESTTMP/hgcache/master/packs/3266aa7480df06153adccad2f1abb6d11f42de0e.datapack
  $TESTTMP/hgcache/master/packs/3b65e3071e408ff050835eba9d2662d0c5ea51db.histidx
  $TESTTMP/hgcache/master/packs/3b65e3071e408ff050835eba9d2662d0c5ea51db.histpack
  $TESTTMP/hgcache/master/packs/887690f1138ae5b99c50d754ed02262874bf8ecb.dataidx
  $TESTTMP/hgcache/master/packs/887690f1138ae5b99c50d754ed02262874bf8ecb.datapack
  $TESTTMP/hgcache/master/packs/acb190832c13f0a23d7901bc1847ef7f6046a26e.histidx
  $TESTTMP/hgcache/master/packs/acb190832c13f0a23d7901bc1847ef7f6046a26e.histpack
  $TESTTMP/hgcache/master/packs/c3399b56e035f73c3295276ed098235a08a0ed8c.histidx
  $TESTTMP/hgcache/master/packs/c3399b56e035f73c3295276ed098235a08a0ed8c.histpack

  $ hg repack --incremental
  $ ls_l $TESTTMP/hgcache/master/packs/ | grep datapack
  -r--r--r--     253 3bf5dfc3af16ed3945192b4a0d53a8188aa2187b.datapack
  $ ls_l $TESTTMP/hgcache/master/packs/ | grep histpack
  -r--r--r--     336 3b65e3071e408ff050835eba9d2662d0c5ea51db.histpack

  $ hg cat -r . x
  x4
  $ hg cat -r '.^' x
  x3
