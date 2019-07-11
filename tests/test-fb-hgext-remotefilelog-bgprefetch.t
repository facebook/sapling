  $ setconfig extensions.treemanifest=!
TODO: Make this test compatibile with obsstore enabled.
  $ setconfig experimental.evolution=

  $ . "$TESTDIR/library.sh"

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > server=True
  > EOF
  $ echo x > x
  $ echo z > z
  $ hg commit -qAm x
  $ echo x2 > x
  $ echo y > y
  $ hg commit -qAm y
  $ echo w > w
  $ rm z
  $ hg commit -qAm w
  $ hg bookmark foo

  $ cd ..

# clone the repo

  $ hgcloneshallow ssh://user@dummy/master shallow --noupdate
  streaming all changes
  2 files to transfer, * bytes of data (glob)
  transferred * bytes in * seconds (*) (glob)
  searching for changes
  no changes found

# Set the prefetchdays config to zero so that all commits are prefetched
# no matter what their creation date is. Also set prefetchdelay config
# to zero so that there is no delay between prefetches.
  $ cd shallow
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > prefetchdays=0
  > prefetchdelay=0
  > EOF
  $ cd ..

# prefetch a revision
  $ cd shallow

  $ hg prefetch -r 0
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over *s (glob)

  $ hg cat -r 0 x
  x

# background prefetch on pull when configured

  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > pullprefetch=bookmark()
  > backgroundprefetch=True
  > EOF
  $ hg debugstrip tip
  saved backup bundle to $TESTTMP/shallow/.hg/strip-backup/6b4b6f66ef8c-b4b8bdaf-backup.hg (glob)

  $ clearcache
  $ hg pull
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  updating bookmark foo
  new changesets 6b4b6f66ef8c
  prefetching file contents
  $ sleep 0.5
  $ hg debugwaitonprefetch >/dev/null 2>%1
  $ find $CACHEDIR -type f | sort
  $TESTTMP/hgcache/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/ef95c5376f34698742fe34f315fd82136f8f68c0
  $TESTTMP/hgcache/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/filename
  $TESTTMP/hgcache/master/95/cb0bfd2977c761298d9624e4b4d4c72a39974a/076f5e2225b3ff0400b98c92aa6cdf403ee24cca
  $TESTTMP/hgcache/master/95/cb0bfd2977c761298d9624e4b4d4c72a39974a/filename
  $TESTTMP/hgcache/master/af/f024fe4ab0fece4091de044c58c9ae4233383a/bb6ccd5dceaa5e9dc220e0dad65e051b94f69a2c
  $TESTTMP/hgcache/master/af/f024fe4ab0fece4091de044c58c9ae4233383a/filename
  $TESTTMP/hgcache/repos

# prefetch uses the current commit as the base
  $ hg up -q 'tip^'
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ clearcache
  $ hg prefetch
  4 files fetched over 1 fetches - (4 misses, 0.00% hit ratio) over * (glob)
  $ clearcache
  $ hg pull -q
  $ sleep 0.5
  $ hg debugwaitonprefetch >/dev/null 2>%1
- Note how the second prefetch only downloads 3 files instead of 4, because the
- background prefetch downloaded the difference between . and the prefetch
- revset.
  $ hg prefetch
  3 files fetched over 1 fetches - (3 misses, 0.00% hit ratio) over * (glob)
  $ hg up -q null

# background prefetch with repack on pull when configured

  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > backgroundrepack=True
  > incrementalloosefilerepack=False
  > EOF
  $ hg debugstrip tip
  saved backup bundle to $TESTTMP/shallow/.hg/strip-backup/6b4b6f66ef8c-b4b8bdaf-backup.hg (glob)

  $ clearcache
  $ hg pull
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  updating bookmark foo
  new changesets 6b4b6f66ef8c
  prefetching file contents
  $ sleep 0.5
  $ hg debugwaitonprefetch >/dev/null 2>%1
  $ sleep 0.5
  $ hg debugwaitonrepack >/dev/null 2>%1
  $ find $CACHEDIR -type f | sort
  $TESTTMP/hgcache/master/packs/94d53eef9e622533aec1fc6d8053cb086e785d21.histidx
  $TESTTMP/hgcache/master/packs/94d53eef9e622533aec1fc6d8053cb086e785d21.histpack
  $TESTTMP/hgcache/master/packs/f3644bc7773e8289deda7f765138120c838f4e6e.dataidx
  $TESTTMP/hgcache/master/packs/f3644bc7773e8289deda7f765138120c838f4e6e.datapack
  $TESTTMP/hgcache/master/packs/repacklock
  $TESTTMP/hgcache/repos

# background prefetch with repack on update when wcprevset configured

  $ clearcache
  $ hg up -r 0
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over *s (glob)
  $ find $CACHEDIR -type f | sort
  $TESTTMP/hgcache/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/1406e74118627694268417491f018a4a883152f0
  $TESTTMP/hgcache/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/filename
  $TESTTMP/hgcache/master/39/5df8f7c51f007019cb30201c49e884b46b92fa/69a1b67522704ec122181c0890bd16e9d3e7516a
  $TESTTMP/hgcache/master/39/5df8f7c51f007019cb30201c49e884b46b92fa/filename
  $TESTTMP/hgcache/repos

  $ hg up -r 1
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  2 files fetched over 2 fetches - (2 misses, 0.00% hit ratio) over *s (glob)

  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > bgprefetchrevs=.::
  > EOF

  $ clearcache
  $ hg up -r 0
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  * files fetched over * fetches - (* misses, 0.00% hit ratio) over *s (glob)
  $ sleep 1
  $ hg debugwaitonprefetch >/dev/null 2>%1
  $ sleep 1
  $ hg debugwaitonrepack >/dev/null 2>%1
  $ find $CACHEDIR -type f | sort
  $TESTTMP/hgcache/master/packs/27c52c105a1ddf8c75143a6b279b04c24b1f4bee.histidx
  $TESTTMP/hgcache/master/packs/27c52c105a1ddf8c75143a6b279b04c24b1f4bee.histpack
  $TESTTMP/hgcache/master/packs/8299d5a1030f073f4adbb3b6bd2ad3bdcc276df0.dataidx
  $TESTTMP/hgcache/master/packs/8299d5a1030f073f4adbb3b6bd2ad3bdcc276df0.datapack
  $TESTTMP/hgcache/master/packs/repacklock
  $TESTTMP/hgcache/repos

# Ensure that file 'w' was prefetched - it was not part of the update operation and therefore
# could only be downloaded by the background prefetch

  $ hg debugdatapack $TESTTMP/hgcache/master/packs/8299d5a1030f073f4adbb3b6bd2ad3bdcc276df0.datapack
  $TESTTMP/hgcache/master/packs/8299d5a1030f073f4adbb3b6bd2ad3bdcc276df0:
  w:
  Node          Delta Base    Delta Length  Blob Size
  bb6ccd5dceaa  000000000000  2             2
  
  Total:                      2             2         (0.0% bigger)
  x:
  Node          Delta Base    Delta Length  Blob Size
  ef95c5376f34  000000000000  3             3
  1406e7411862  ef95c5376f34  14            2
  
  Total:                      17            5         (240.0% bigger)
  y:
  Node          Delta Base    Delta Length  Blob Size
  076f5e2225b3  000000000000  2             2
  
  Total:                      2             2         (0.0% bigger)
  z:
  Node          Delta Base    Delta Length  Blob Size
  69a1b6752270  000000000000  2             2
  
  Total:                      2             2         (0.0% bigger)

# background prefetch with repack on commit when wcprevset configured

  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > bgprefetchrevs=0::
  > EOF

  $ sleep 0.5
  $ clearcache
  $ find $CACHEDIR -type f | sort
  $ echo b > b
  $ hg commit -qAm b
  * files fetched over 1 fetches - (* misses, 0.00% hit ratio) over *s (glob)
  $ hg bookmark temporary
  $ sleep 1
  $ hg debugwaitonprefetch >/dev/null 2>%1
  $ sleep 1
  $ hg debugwaitonrepack >/dev/null 2>%1
  $ find $CACHEDIR -type f | sort
  $TESTTMP/hgcache/master/packs/27c52c105a1ddf8c75143a6b279b04c24b1f4bee.histidx
  $TESTTMP/hgcache/master/packs/27c52c105a1ddf8c75143a6b279b04c24b1f4bee.histpack
  $TESTTMP/hgcache/master/packs/8299d5a1030f073f4adbb3b6bd2ad3bdcc276df0.dataidx
  $TESTTMP/hgcache/master/packs/8299d5a1030f073f4adbb3b6bd2ad3bdcc276df0.datapack
  $TESTTMP/hgcache/master/packs/repacklock
  $TESTTMP/hgcache/repos

# Ensure that file 'w' was prefetched - it was not part of the commit operation and therefore
# could only be downloaded by the background prefetch

  $ hg debugdatapack $TESTTMP/hgcache/master/packs/8299d5a1030f073f4adbb3b6bd2ad3bdcc276df0.datapack
  $TESTTMP/hgcache/master/packs/8299d5a1030f073f4adbb3b6bd2ad3bdcc276df0:
  w:
  Node          Delta Base    Delta Length  Blob Size
  bb6ccd5dceaa  000000000000  2             2
  
  Total:                      2             2         (0.0% bigger)
  x:
  Node          Delta Base    Delta Length  Blob Size
  ef95c5376f34  000000000000  3             3
  1406e7411862  ef95c5376f34  14            2
  
  Total:                      17            5         (240.0% bigger)
  y:
  Node          Delta Base    Delta Length  Blob Size
  076f5e2225b3  000000000000  2             2
  
  Total:                      2             2         (0.0% bigger)
  z:
  Node          Delta Base    Delta Length  Blob Size
  69a1b6752270  000000000000  2             2
  
  Total:                      2             2         (0.0% bigger)

# background prefetch with repack on rebase when wcprevset configured

  $ hg up -r 2
  3 files updated, 0 files merged, 3 files removed, 0 files unresolved
  (leaving bookmark temporary)
  $ sleep 0.5 # wait for any prefetches triggered by update
  $ hg debugwaitonprefetch >/dev/null 2>%1
  $ sleep 0.5
  $ hg debugwaitonrepack >/dev/null 2>%1
  $ clearcache
  $ find $CACHEDIR -type f | sort
  $ hg rebase -s temporary -d foo
  rebasing 3:58147a5b5242 "b" (temporary tip)
  saved backup bundle to $TESTTMP/shallow/.hg/strip-backup/58147a5b5242-c3678817-rebase.hg (glob)
  3 files fetched over 1 fetches - (3 misses, 0.00% hit ratio) over *s (glob)
  $ sleep 1
  $ hg debugwaitonprefetch >/dev/null 2>%1
  $ sleep 1
  $ hg debugwaitonrepack >/dev/null 2>%1

# Ensure that file 'y' was prefetched - it was not part of the rebase operation and therefore
# could only be downloaded by the background prefetch

  $ hg debugdatapack $TESTTMP/hgcache/master/packs/8299d5a1030f073f4adbb3b6bd2ad3bdcc276df0.datapack
  $TESTTMP/hgcache/master/packs/8299d5a1030f073f4adbb3b6bd2ad3bdcc276df0:
  w:
  Node          Delta Base    Delta Length  Blob Size
  bb6ccd5dceaa  000000000000  2             2
  
  Total:                      2             2         (0.0% bigger)
  x:
  Node          Delta Base    Delta Length  Blob Size
  ef95c5376f34  000000000000  3             3
  1406e7411862  ef95c5376f34  14            2
  
  Total:                      17            5         (240.0% bigger)
  y:
  Node          Delta Base    Delta Length  Blob Size
  076f5e2225b3  000000000000  2             2
  
  Total:                      2             2         (0.0% bigger)
  z:
  Node          Delta Base    Delta Length  Blob Size
  69a1b6752270  000000000000  2             2
  
  Total:                      2             2         (0.0% bigger)

# Check that foregound prefetch with no arguments blocks until background prefetches finish

  $ hg up -r 3
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ clearcache
  $ hg prefetch --repack 2>&1 | grep 'got lock' || true
  got lock after * seconds (glob) (?)

  $ sleep 0.5
  $ hg debugwaitonrepack >/dev/null 2>%1

  $ find $CACHEDIR -type f | sort
  $TESTTMP/hgcache/master/packs/27c52c105a1ddf8c75143a6b279b04c24b1f4bee.histidx
  $TESTTMP/hgcache/master/packs/27c52c105a1ddf8c75143a6b279b04c24b1f4bee.histpack
  $TESTTMP/hgcache/master/packs/8299d5a1030f073f4adbb3b6bd2ad3bdcc276df0.dataidx
  $TESTTMP/hgcache/master/packs/8299d5a1030f073f4adbb3b6bd2ad3bdcc276df0.datapack
  $TESTTMP/hgcache/master/packs/repacklock
  $TESTTMP/hgcache/repos

# Ensure that files were prefetched
  $ hg debugdatapack $TESTTMP/hgcache/master/packs/8299d5a1030f073f4adbb3b6bd2ad3bdcc276df0.datapack
  $TESTTMP/hgcache/master/packs/8299d5a1030f073f4adbb3b6bd2ad3bdcc276df0:
  w:
  Node          Delta Base    Delta Length  Blob Size
  bb6ccd5dceaa  000000000000  2             2
  
  Total:                      2             2         (0.0% bigger)
  x:
  Node          Delta Base    Delta Length  Blob Size
  ef95c5376f34  000000000000  3             3
  1406e7411862  ef95c5376f34  14            2
  
  Total:                      17            5         (240.0% bigger)
  y:
  Node          Delta Base    Delta Length  Blob Size
  076f5e2225b3  000000000000  2             2
  
  Total:                      2             2         (0.0% bigger)
  z:
  Node          Delta Base    Delta Length  Blob Size
  69a1b6752270  000000000000  2             2
  
  Total:                      2             2         (0.0% bigger)

# Check that foreground prefetch fetches revs specified by '. + draft() + bgprefetchrevs + pullprefetch'

  $ clearcache
  $ hg prefetch --repack 2>&1 | grep 'got lock' || true
  got lock after * seconds (glob) (?)
  $ sleep 0.5
  $ hg debugwaitonrepack >/dev/null 2>%1

  $ find $CACHEDIR -type f | sort
  $TESTTMP/hgcache/master/packs/27c52c105a1ddf8c75143a6b279b04c24b1f4bee.histidx
  $TESTTMP/hgcache/master/packs/27c52c105a1ddf8c75143a6b279b04c24b1f4bee.histpack
  $TESTTMP/hgcache/master/packs/8299d5a1030f073f4adbb3b6bd2ad3bdcc276df0.dataidx
  $TESTTMP/hgcache/master/packs/8299d5a1030f073f4adbb3b6bd2ad3bdcc276df0.datapack
  $TESTTMP/hgcache/master/packs/repacklock
  $TESTTMP/hgcache/repos

# Ensure that files were prefetched
  $ hg debugdatapack $TESTTMP/hgcache/master/packs/8299d5a1030f073f4adbb3b6bd2ad3bdcc276df0.datapack
  $TESTTMP/hgcache/master/packs/8299d5a1030f073f4adbb3b6bd2ad3bdcc276df0:
  w:
  Node          Delta Base    Delta Length  Blob Size
  bb6ccd5dceaa  000000000000  2             2
  
  Total:                      2             2         (0.0% bigger)
  x:
  Node          Delta Base    Delta Length  Blob Size
  ef95c5376f34  000000000000  3             3
  1406e7411862  ef95c5376f34  14            2
  
  Total:                      17            5         (240.0% bigger)
  y:
  Node          Delta Base    Delta Length  Blob Size
  076f5e2225b3  000000000000  2             2
  
  Total:                      2             2         (0.0% bigger)
  z:
  Node          Delta Base    Delta Length  Blob Size
  69a1b6752270  000000000000  2             2
  
  Total:                      2             2         (0.0% bigger)

# Test that if data was prefetched and repacked we dont need to prefetch it again
# It ensures that Mercurial looks not only in loose files but in packs as well

  $ hg prefetch --repack
  (running background incremental repack)
