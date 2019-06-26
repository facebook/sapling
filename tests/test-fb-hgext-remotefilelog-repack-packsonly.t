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
  $ echo x >> x
  $ hg commit -qAm x2
  $ cd ..

  $ hgcloneshallow ssh://user@dummy/master shallow -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)

# Set the prefetchdays config to zero so that all commits are prefetched
# no matter what their creation date is.
  $ cd shallow
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > prefetchdays=0
  > userustrepack=True
  > fetchpacks=True
  > EOF
  $ cd ..

  $ cd shallow
  $ find $CACHEDIR | sort
  $TESTTMP/hgcache
  $TESTTMP/hgcache/master
  $TESTTMP/hgcache/master/11
  $TESTTMP/hgcache/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072
  $TESTTMP/hgcache/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/aee31534993a501858fb6dd96a065671922e7d51
  $TESTTMP/hgcache/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/filename
  $TESTTMP/hgcache/repos

  $ cd ../master
  $ echo x3 > x
  $ hg commit -qAm x3
  $ echo x4 > x
  $ hg commit -qAm x4
  $ cd ../shallow
  $ hg pull -q
  $ hg up -q tip
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)
  $ hg prefetch -r 0
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ hg prefetch -r 2
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ find $CACHEDIR | sort
  $TESTTMP/hgcache
  $TESTTMP/hgcache/master
  $TESTTMP/hgcache/master/11
  $TESTTMP/hgcache/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072
  $TESTTMP/hgcache/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/aee31534993a501858fb6dd96a065671922e7d51
  $TESTTMP/hgcache/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/filename
  $TESTTMP/hgcache/master/packs
  $TESTTMP/hgcache/master/packs/1e6f0f575de6319f747ef83966a08775803fcecc.dataidx
  $TESTTMP/hgcache/master/packs/1e6f0f575de6319f747ef83966a08775803fcecc.datapack
  $TESTTMP/hgcache/master/packs/2d66e09c3bf8a000428af1630d978127182e496e.dataidx
  $TESTTMP/hgcache/master/packs/2d66e09c3bf8a000428af1630d978127182e496e.datapack
  $TESTTMP/hgcache/master/packs/3266aa7480df06153adccad2f1abb6d11f42de0e.dataidx
  $TESTTMP/hgcache/master/packs/3266aa7480df06153adccad2f1abb6d11f42de0e.datapack
  $TESTTMP/hgcache/master/packs/3b65e3071e408ff050835eba9d2662d0c5ea51db.histidx
  $TESTTMP/hgcache/master/packs/3b65e3071e408ff050835eba9d2662d0c5ea51db.histpack
  $TESTTMP/hgcache/master/packs/acb190832c13f0a23d7901bc1847ef7f6046a26e.histidx
  $TESTTMP/hgcache/master/packs/acb190832c13f0a23d7901bc1847ef7f6046a26e.histpack
  $TESTTMP/hgcache/master/packs/c3399b56e035f73c3295276ed098235a08a0ed8c.histidx
  $TESTTMP/hgcache/master/packs/c3399b56e035f73c3295276ed098235a08a0ed8c.histpack
  $TESTTMP/hgcache/repos

  $ hg pull -q --config remotefilelog.backgroundrepack=True --config remotefilelog.packsonlyrepack=True
  (running background incremental repack)
  $ sleep 0.5
  $ hg debugwaitonprefetch >/dev/null 2>%1

  $ find $CACHEDIR | sort
  $TESTTMP/hgcache
  $TESTTMP/hgcache/master
  $TESTTMP/hgcache/master/11
  $TESTTMP/hgcache/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072
  $TESTTMP/hgcache/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/aee31534993a501858fb6dd96a065671922e7d51
  $TESTTMP/hgcache/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/filename
  $TESTTMP/hgcache/master/packs
  $TESTTMP/hgcache/master/packs/3b65e3071e408ff050835eba9d2662d0c5ea51db.histidx
  $TESTTMP/hgcache/master/packs/3b65e3071e408ff050835eba9d2662d0c5ea51db.histpack
  $TESTTMP/hgcache/master/packs/591442b244487739875c540ec84be5a499a0b069.dataidx
  $TESTTMP/hgcache/master/packs/591442b244487739875c540ec84be5a499a0b069.datapack
  $TESTTMP/hgcache/master/packs/repacklock
  $TESTTMP/hgcache/repos
