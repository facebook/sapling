  $ setconfig extensions.treemanifest=!

  $ . "$TESTDIR/library.sh"

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > server=True
  > EOF
  $ echo x > x
  $ hg commit -qAm x
  $ echo y > y
  $ rm x
  $ hg commit -qAm DxAy
  $ echo yy > y
  $ hg commit -qAm y
  $ cd ..

  $ hgcloneshallow ssh://user@dummy/master shallow -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)

# Set the prefetchdays config to zero so that all commits are prefetched
# no matter what their creation date is.
  $ cd shallow
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > prefetchdays=0
  > EOF
  $ cd ..

# Prefetch all data and repack

  $ cd shallow
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > bgprefetchrevs=all()
  > EOF

  $ hg prefetch
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over *s (glob)
  $ hg repack
  $ sleep 0.5
  $ hg debugwaitonrepack >/dev/null 2>%1

  $ find $CACHEDIR | sort | grep ".datapack\|.histpack"
  $TESTTMP/hgcache/master/packs/9a2ea858fe2967db9b6ea4c0ca238881cae9d6eb.histpack
  $TESTTMP/hgcache/master/packs/f7a942a6e4673d2c7b697fdd926ca2d153831ca4.datapack

# Ensure that all file versions were prefetched

  $ hg debugdatapack $TESTTMP/hgcache/master/packs/f7a942a6e4673d2c7b697fdd926ca2d153831ca4.datapack
  $TESTTMP/hgcache/master/packs/f7a942a6e4673d2c7b697fdd926ca2d153831ca4:
  x:
  Node          Delta Base    Delta Length  Blob Size
  1406e7411862  000000000000  2             2
  
  Total:                      2             2         (0.0% bigger)
  y:
  Node          Delta Base    Delta Length  Blob Size
  50dbc4572b8e  000000000000  3             3
  076f5e2225b3  50dbc4572b8e  14            2
  
  Total:                      17            5         (240.0% bigger)

# Test garbage collection during repack

  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > bgprefetchrevs=tip
  > gcrepack=True
  > nodettl=86400
  > EOF

  $ hg repack
  $ sleep 0.5
  $ hg debugwaitonrepack >/dev/null 2>%1

  $ find $CACHEDIR | sort | grep ".datapack\|.histpack"
  $TESTTMP/hgcache/master/packs/05baa499c6b07f2bf0ea3d2c8151da1cb86f5e33.datapack
  $TESTTMP/hgcache/master/packs/9a2ea858fe2967db9b6ea4c0ca238881cae9d6eb.histpack

# Ensure that file 'x' was garbage collected. It should be GCed because it is not in the keepset
# and is old (commit date is 0.0 in tests). Ensure that file 'y' is present as it is in the keepset.

  $ hg debugdatapack $TESTTMP/hgcache/master/packs/05baa499c6b07f2bf0ea3d2c8151da1cb86f5e33.datapack
  $TESTTMP/hgcache/master/packs/05baa499c6b07f2bf0ea3d2c8151da1cb86f5e33:
  y:
  Node          Delta Base    Delta Length  Blob Size
  50dbc4572b8e  000000000000  3             3
  
  Total:                      3             3         (0.0% bigger)

# Prefetch all data again and repack for later garbage collection

  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > bgprefetchrevs=all()
  > EOF

  $ hg prefetch
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over *s (glob)
  $ hg repack
  $ sleep 0.5
  $ hg debugwaitonrepack >/dev/null 2>%1

  $ find $CACHEDIR | sort | grep ".datapack\|.histpack"
  $TESTTMP/hgcache/master/packs/9a2ea858fe2967db9b6ea4c0ca238881cae9d6eb.histpack
  $TESTTMP/hgcache/master/packs/f7a942a6e4673d2c7b697fdd926ca2d153831ca4.datapack

# Ensure that all file versions were prefetched

  $ hg debugdatapack $TESTTMP/hgcache/master/packs/f7a942a6e4673d2c7b697fdd926ca2d153831ca4.datapack
  $TESTTMP/hgcache/master/packs/f7a942a6e4673d2c7b697fdd926ca2d153831ca4:
  x:
  Node          Delta Base    Delta Length  Blob Size
  1406e7411862  000000000000  2             2
  
  Total:                      2             2         (0.0% bigger)
  y:
  Node          Delta Base    Delta Length  Blob Size
  50dbc4572b8e  000000000000  3             3
  076f5e2225b3  50dbc4572b8e  14            2
  
  Total:                      17            5         (240.0% bigger)

# Test garbage collection during repack. Ensure that new files are not removed even though they are not in the keepset
# For the purposes of the test the TTL of a file is set to current time + 100 seconds. i.e. all commits in tests have
# a date of 1970 and therefore to prevent garbage collection we have to set nodettl to be farther from 1970 than we are now.

  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > bgprefetchrevs=
  > nodettl=$(($(date +%s) + 100))
  > EOF

  $ hg repack
  $ sleep 0.5
  $ hg debugwaitonrepack >/dev/null 2>%1

  $ find $CACHEDIR | sort | grep ".datapack\|.histpack"
  $TESTTMP/hgcache/master/packs/9a2ea858fe2967db9b6ea4c0ca238881cae9d6eb.histpack
  $TESTTMP/hgcache/master/packs/f7a942a6e4673d2c7b697fdd926ca2d153831ca4.datapack

# Ensure that all file versions were prefetched

  $ hg debugdatapack $TESTTMP/hgcache/master/packs/f7a942a6e4673d2c7b697fdd926ca2d153831ca4.datapack
  $TESTTMP/hgcache/master/packs/f7a942a6e4673d2c7b697fdd926ca2d153831ca4:
  x:
  Node          Delta Base    Delta Length  Blob Size
  1406e7411862  000000000000  2             2
  
  Total:                      2             2         (0.0% bigger)
  y:
  Node          Delta Base    Delta Length  Blob Size
  50dbc4572b8e  000000000000  3             3
  076f5e2225b3  50dbc4572b8e  14            2
  
  Total:                      17            5         (240.0% bigger)
