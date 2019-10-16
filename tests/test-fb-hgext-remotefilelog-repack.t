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
  > localdatarepack=True
  > prefetchdays=0
  > EOF
  $ cd ..

# Test that repack cleans up the old files and creates new packs
# metrics should show the total size and counf of the loose files

  $ cd shallow
  $ find $CACHEDIR | sort
  $TESTTMP/hgcache
  $TESTTMP/hgcache/master
  $TESTTMP/hgcache/master/packs
  $TESTTMP/hgcache/master/packs/276d308429d0303762befa376788300f0310f90e.histidx
  $TESTTMP/hgcache/master/packs/276d308429d0303762befa376788300f0310f90e.histpack
  $TESTTMP/hgcache/master/packs/887690f1138ae5b99c50d754ed02262874bf8ecb.dataidx
  $TESTTMP/hgcache/master/packs/887690f1138ae5b99c50d754ed02262874bf8ecb.datapack

  $ hg repack --config "devel.print-metrics=1"

  $ find $CACHEDIR | sort
  $TESTTMP/hgcache
  $TESTTMP/hgcache/master
  $TESTTMP/hgcache/master/packs
  $TESTTMP/hgcache/master/packs/276d308429d0303762befa376788300f0310f90e.histidx
  $TESTTMP/hgcache/master/packs/276d308429d0303762befa376788300f0310f90e.histpack
  $TESTTMP/hgcache/master/packs/887690f1138ae5b99c50d754ed02262874bf8ecb.dataidx
  $TESTTMP/hgcache/master/packs/887690f1138ae5b99c50d754ed02262874bf8ecb.datapack
  $TESTTMP/hgcache/master/packs/repacklock

# Test that the packs are readonly
  $ ls_l $CACHEDIR/master/packs
  -r--r--r--    1145 276d308429d0303762befa376788300f0310f90e.histidx
  -r--r--r--     172 276d308429d0303762befa376788300f0310f90e.histpack
  -r--r--r--    1074 887690f1138ae5b99c50d754ed02262874bf8ecb.dataidx
  -r--r--r--      65 887690f1138ae5b99c50d754ed02262874bf8ecb.datapack
  -rw-rw-r--       0 repacklock

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
  $TESTTMP/hgcache/master/packs/077e7ce5dfe862dc40cc8f3c9742d96a056865f2.histidx
  $TESTTMP/hgcache/master/packs/077e7ce5dfe862dc40cc8f3c9742d96a056865f2.histpack
  $TESTTMP/hgcache/master/packs/276d308429d0303762befa376788300f0310f90e.histidx
  $TESTTMP/hgcache/master/packs/276d308429d0303762befa376788300f0310f90e.histpack
  $TESTTMP/hgcache/master/packs/384bdd4f40c2a4fc8f60ab4612381b9052ebe917.dataidx
  $TESTTMP/hgcache/master/packs/384bdd4f40c2a4fc8f60ab4612381b9052ebe917.datapack
  $TESTTMP/hgcache/master/packs/887690f1138ae5b99c50d754ed02262874bf8ecb.dataidx
  $TESTTMP/hgcache/master/packs/887690f1138ae5b99c50d754ed02262874bf8ecb.datapack
  $TESTTMP/hgcache/master/packs/repacklock

# First assert that with --packsonly, the loose object will be ignored:

  $ hg repack --packsonly --config "devel.print-metrics=1"

  $ find $CACHEDIR -type f | sort
  $TESTTMP/hgcache/master/packs/077e7ce5dfe862dc40cc8f3c9742d96a056865f2.histidx
  $TESTTMP/hgcache/master/packs/077e7ce5dfe862dc40cc8f3c9742d96a056865f2.histpack
  $TESTTMP/hgcache/master/packs/fe74acc023664cd7eda12330d321b4a2423cc4f8.dataidx
  $TESTTMP/hgcache/master/packs/fe74acc023664cd7eda12330d321b4a2423cc4f8.datapack
  $TESTTMP/hgcache/master/packs/repacklock

# Now test that --looseonly will only repack the loose file, leaving
# the old packs:

  $ hg repack --looseonly --traceback --config "devel.print-metrics=1"

  $ find $CACHEDIR -type f | sort
  $TESTTMP/hgcache/master/packs/077e7ce5dfe862dc40cc8f3c9742d96a056865f2.histidx
  $TESTTMP/hgcache/master/packs/077e7ce5dfe862dc40cc8f3c9742d96a056865f2.histpack
  $TESTTMP/hgcache/master/packs/fe74acc023664cd7eda12330d321b4a2423cc4f8.dataidx
  $TESTTMP/hgcache/master/packs/fe74acc023664cd7eda12330d321b4a2423cc4f8.datapack
  $TESTTMP/hgcache/master/packs/repacklock

# A full repack creates the optimal packing:

  $ hg repack --traceback --config "devel.print-metrics=1"

  $ find $CACHEDIR -type f | sort
  $TESTTMP/hgcache/master/packs/077e7ce5dfe862dc40cc8f3c9742d96a056865f2.histidx
  $TESTTMP/hgcache/master/packs/077e7ce5dfe862dc40cc8f3c9742d96a056865f2.histpack
  $TESTTMP/hgcache/master/packs/fe74acc023664cd7eda12330d321b4a2423cc4f8.dataidx
  $TESTTMP/hgcache/master/packs/fe74acc023664cd7eda12330d321b4a2423cc4f8.datapack
  $TESTTMP/hgcache/master/packs/repacklock

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
  $TESTTMP/hgcache/master/packs/077e7ce5dfe862dc40cc8f3c9742d96a056865f2.histidx
  $TESTTMP/hgcache/master/packs/077e7ce5dfe862dc40cc8f3c9742d96a056865f2.histpack
  $TESTTMP/hgcache/master/packs/fe74acc023664cd7eda12330d321b4a2423cc4f8.dataidx
  $TESTTMP/hgcache/master/packs/fe74acc023664cd7eda12330d321b4a2423cc4f8.datapack
  $TESTTMP/hgcache/master/packs/repacklock

# Run two repacks at once
  $ hg repack --config "hooks.prerepack=sleep 3" &
  $ sleep 1
  $ hg repack
  skipping repack - another repack is already running
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
  $TESTTMP/hgcache/master/packs/077e7ce5dfe862dc40cc8f3c9742d96a056865f2.histidx
  $TESTTMP/hgcache/master/packs/077e7ce5dfe862dc40cc8f3c9742d96a056865f2.histpack
  $TESTTMP/hgcache/master/packs/094b530486dad4427a0faf6bcbc031571b99ca24.histidx
  $TESTTMP/hgcache/master/packs/094b530486dad4427a0faf6bcbc031571b99ca24.histpack
  $TESTTMP/hgcache/master/packs/20e1a91049a48630732a9b2ceaf4fa783a3af6c5.dataidx
  $TESTTMP/hgcache/master/packs/20e1a91049a48630732a9b2ceaf4fa783a3af6c5.datapack
  $TESTTMP/hgcache/master/packs/fe74acc023664cd7eda12330d321b4a2423cc4f8.dataidx
  $TESTTMP/hgcache/master/packs/fe74acc023664cd7eda12330d321b4a2423cc4f8.datapack
  $TESTTMP/hgcache/master/packs/repacklock

  $ hg repack --background
  (running background repack)
  $ sleep 0.5
  $ hg debugwaitonrepack >/dev/null 2>&1
  $ find $CACHEDIR -type f | sort
  $TESTTMP/hgcache/master/packs/094b530486dad4427a0faf6bcbc031571b99ca24.histidx
  $TESTTMP/hgcache/master/packs/094b530486dad4427a0faf6bcbc031571b99ca24.histpack
  $TESTTMP/hgcache/master/packs/d16759ce28060a639a43d64b9a243e4f86b26a46.dataidx
  $TESTTMP/hgcache/master/packs/d16759ce28060a639a43d64b9a243e4f86b26a46.datapack
  $TESTTMP/hgcache/master/packs/repacklock

# Test debug commands

  $ hg debugdatapack $TESTTMP/hgcache/master/packs/*.datapack
  $TESTTMP/hgcache/master/packs/d16759ce28060a639a43d64b9a243e4f86b26a46:
  x:
  Node          Delta Base    Delta Length  Blob Size
  1bb2e6237e03  000000000000  8             (missing)
  d4a3ed9310e5  000000000000  6             (missing)
  aee31534993a  000000000000  4             (missing)
  
  $ hg debugdatapack --long $TESTTMP/hgcache/master/packs/*.datapack
  $TESTTMP/hgcache/master/packs/d16759ce28060a639a43d64b9a243e4f86b26a46:
  x:
  Node                                      Delta Base                                Delta Length  Blob Size
  1bb2e6237e035c8f8ef508e281f1ce075bc6db72  0000000000000000000000000000000000000000  8             (missing)
  d4a3ed9310e5bd9887e3bf779da5077efab28216  0000000000000000000000000000000000000000  6             (missing)
  aee31534993a501858fb6dd96a065671922e7d51  0000000000000000000000000000000000000000  4             (missing)
  
  $ hg debugdatapack $TESTTMP/hgcache/master/packs/*.datapack --node-delta d4a3ed9310e5bd9887e3bf779da5077efab28216
  $TESTTMP/hgcache/master/packs/d16759ce28060a639a43d64b9a243e4f86b26a46:
  
  x
  Node                                      Delta Base                                Delta SHA1                                Delta Length
  d4a3ed9310e5bd9887e3bf779da5077efab28216  0000000000000000000000000000000000000000  86e1b96c0167d9d28fe9bd21ea208d21033c869c  6
  $ hg debughistorypack $TESTTMP/hgcache/master/packs/*.histidx
  
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
  $ echo >> y
  $ hg revert x
  $ hg commit -m 'add y back without metadata'
  $ cd ../shallow
  $ hg pull -q
  $ hg up -q tip
  2 files fetched over 2 fetches - (2 misses, 0.00% hit ratio) over * (glob)
  $ hg repack
  $ ls $TESTTMP/hgcache/master/packs
  73d5a02cfb63aa0417c03a9cb2ba8504bb6f312b.dataidx
  73d5a02cfb63aa0417c03a9cb2ba8504bb6f312b.datapack
  ebbd7411e00456c0eec8d1150a77e2b3ef490f3f.histidx
  ebbd7411e00456c0eec8d1150a77e2b3ef490f3f.histpack
  repacklock
  $ hg debughistorypack $TESTTMP/hgcache/master/packs/*.histidx
  
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
  21f46f2721e7  000000000000  000000000000  d6868642b790  
  $ hg debugstrip -r '.^'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/shallow/.hg/strip-backup/609547eda446-b26b56a8-backup.hg (glob)
  $ hg -R ../master debugstrip -r '.^'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/master/.hg/strip-backup/609547eda446-b26b56a8-backup.hg (glob)

  $ rm -rf $TESTTMP/hgcache/master/packs
  $ cp -R $TESTTMP/backuppacks $TESTTMP/hgcache/master/packs

# Test repacking datapack without history
  $ rm -rf $CACHEDIR/master/packs/*hist*
  $ hg repack
  $ hg debugdatapack $TESTTMP/hgcache/master/packs/*.datapack
  $TESTTMP/hgcache/master/packs/4fbad80d995df2af342caa2aea3480958a5601ec:
  y:
  Node          Delta Base    Delta Length  Blob Size
  577959738234  000000000000  70            (missing)
  
  x:
  Node          Delta Base    Delta Length  Blob Size
  1bb2e6237e03  000000000000  8             (missing)
  d4a3ed9310e5  000000000000  6             (missing)
  aee31534993a  000000000000  4             (missing)
  

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
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ ls_l $TESTTMP/hgcache/master/packs/ | grep datapack
  -r--r--r--      63 2d66e09c3bf8a000428af1630d978127182e496e.datapack
  $ ls_l $TESTTMP/hgcache/master/packs/ | grep histpack
  -r--r--r--      90 c3399b56e035f73c3295276ed098235a08a0ed8c.histpack
  $ hg repack --incremental
  $ ls_l $TESTTMP/hgcache/master/packs/ | grep datapack
  -r--r--r--      63 2d66e09c3bf8a000428af1630d978127182e496e.datapack
  $ ls_l $TESTTMP/hgcache/master/packs/ | grep histpack
  -r--r--r--      90 c3399b56e035f73c3295276ed098235a08a0ed8c.histpack

3 gen1 packs, 1 gen0 pack - packs 3 gen1 into 1
  $ hg prefetch -r 1
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ hg prefetch -r 2
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ hg prefetch -r 3
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ ls_l $TESTTMP/hgcache/master/packs/ | grep datapack
  -r--r--r--      69 20e1a91049a48630732a9b2ceaf4fa783a3af6c5.datapack
  -r--r--r--      63 2d66e09c3bf8a000428af1630d978127182e496e.datapack
  -r--r--r--      67 384bdd4f40c2a4fc8f60ab4612381b9052ebe917.datapack
  -r--r--r--      65 887690f1138ae5b99c50d754ed02262874bf8ecb.datapack
  $ ls_l $TESTTMP/hgcache/master/packs/ | grep histpack
  -r--r--r--     254 077e7ce5dfe862dc40cc8f3c9742d96a056865f2.histpack
  -r--r--r--     336 094b530486dad4427a0faf6bcbc031571b99ca24.histpack
  -r--r--r--     172 276d308429d0303762befa376788300f0310f90e.histpack
  -r--r--r--      90 c3399b56e035f73c3295276ed098235a08a0ed8c.histpack

For the data packs, setting the limit for the repackmaxpacksize to be 64 such
that data pack with size 65 is more than the limit. This effectively ensures
that no generation has 3 packs and therefore, no packs are chosen for the
incremental repacking. As for the history packs, setting repackmaxpacksize to be
0 which should always result in no repacking.
  $ hg repack --incremental --config remotefilelog.data.repackmaxpacksize=64 \
  > --config remotefilelog.history.repackmaxpacksize=0
  $ ls_l $TESTTMP/hgcache/master/packs/ | grep datapack
  -r--r--r--     261 c155d24742424ff6f6eec6c54d232c3f550b6922.datapack
  $ ls_l $TESTTMP/hgcache/master/packs/ | grep histpack
  -r--r--r--     336 094b530486dad4427a0faf6bcbc031571b99ca24.histpack

Setting limit for the repackmaxpacksize to be the size of the biggest pack file
which ensures that it is effectively ignored in the incremental repacking.
  $ hg repack --incremental --config remotefilelog.data.repackmaxpacksize=65 \
  > --config remotefilelog.history.repackmaxpacksize=336
  $ ls_l $TESTTMP/hgcache/master/packs/ | grep datapack
  -r--r--r--     261 c155d24742424ff6f6eec6c54d232c3f550b6922.datapack
  $ ls_l $TESTTMP/hgcache/master/packs/ | grep histpack
  -r--r--r--     336 094b530486dad4427a0faf6bcbc031571b99ca24.histpack

1 gen3 pack, 1 gen0 pack - does nothing
  $ hg repack --incremental
  $ ls_l $TESTTMP/hgcache/master/packs/ | grep datapack
  -r--r--r--     261 c155d24742424ff6f6eec6c54d232c3f550b6922.datapack
  $ ls_l $TESTTMP/hgcache/master/packs/ | grep histpack
  -r--r--r--     336 094b530486dad4427a0faf6bcbc031571b99ca24.histpack

Pull should run background repack
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > backgroundrepack=True
  > EOF
  $ clearcache
  $ hg prefetch -r 0
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ hg prefetch -r 1
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ hg prefetch -r 2
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ hg prefetch -r 3
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ ls_l $TESTTMP/hgcache/master/packs/ | grep datapack
  -r--r--r--      69 20e1a91049a48630732a9b2ceaf4fa783a3af6c5.datapack
  -r--r--r--      63 2d66e09c3bf8a000428af1630d978127182e496e.datapack
  -r--r--r--      67 384bdd4f40c2a4fc8f60ab4612381b9052ebe917.datapack
  -r--r--r--      65 887690f1138ae5b99c50d754ed02262874bf8ecb.datapack
  $ ls_l $TESTTMP/hgcache/master/packs/ | grep histpack
  -r--r--r--     254 077e7ce5dfe862dc40cc8f3c9742d96a056865f2.histpack
  -r--r--r--     336 094b530486dad4427a0faf6bcbc031571b99ca24.histpack
  -r--r--r--     172 276d308429d0303762befa376788300f0310f90e.histpack
  -r--r--r--      90 c3399b56e035f73c3295276ed098235a08a0ed8c.histpack

  $ hg pull
  pulling from ssh://user@dummy/master
  searching for changes
  no changes found
  (running background incremental repack)
  $ sleep 0.5
  $ hg debugwaitonrepack >/dev/null 2>&1
  $ ls_l $TESTTMP/hgcache/master/packs/ | grep datapack
  -r--r--r--     261 c155d24742424ff6f6eec6c54d232c3f550b6922.datapack
  $ ls_l $TESTTMP/hgcache/master/packs/ | grep histpack
  -r--r--r--     336 094b530486dad4427a0faf6bcbc031571b99ca24.histpack

Test environment variable resolution
  $ CACHEPATH=$TESTTMP/envcache hg prefetch --config 'remotefilelog.cachepath=$CACHEPATH'
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ find $TESTTMP/envcache | sort
  $TESTTMP/envcache
  $TESTTMP/envcache/master
  $TESTTMP/envcache/master/packs
  $TESTTMP/envcache/master/packs/2021b67b6df3cec03f6ca46b83a3e69a67b204ec.dataidx
  $TESTTMP/envcache/master/packs/2021b67b6df3cec03f6ca46b83a3e69a67b204ec.datapack
  $TESTTMP/envcache/master/packs/dcebd8e8d4d97ee88e40dd8f92d8678c10e1a3ad.histidx
  $TESTTMP/envcache/master/packs/dcebd8e8d4d97ee88e40dd8f92d8678c10e1a3ad.histpack

Test limiting the max delta chain length
  $ hg repack --config packs.maxchainlen=1
  $ hg debugdatapack $TESTTMP/hgcache/master/packs/*.dataidx
  $TESTTMP/hgcache/master/packs/c155d24742424ff6f6eec6c54d232c3f550b6922:
  x:
  Node          Delta Base    Delta Length  Blob Size
  1406e7411862  000000000000  2             (missing)
  aee31534993a  000000000000  4             (missing)
  d4a3ed9310e5  000000000000  6             (missing)
  1bb2e6237e03  000000000000  8             (missing)
  

Test huge pack cleanup using different values of packs.maxpacksize:
  $ hg repack --incremental --debug
  $ hg repack --incremental --debug --config packs.maxpacksize=512
  removing oversize packfile $TESTTMP/hgcache/master/packs/c155d24742424ff6f6eec6c54d232c3f550b6922.datapack (261 bytes)
  removing oversize packfile $TESTTMP/hgcache/master/packs/c155d24742424ff6f6eec6c54d232c3f550b6922.dataidx (1.17 KB)

# Test repacking loose files
  $ findfilessorted .hg/store/data
  $ findfilessorted .hg/store/packs

# new loose file is created
  $ echo "new commit" > new_file
  $ hg commit -qAm "one more node"
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over 0.00s
  $ findfilessorted .hg/store/data

# repacking only loose files
  $ hg repack --looseonly
  $ findfilessorted .hg/store/packs
  .hg/store/packs/114243ab99c9697960e23423df9d98f259a0159d.histidx
  .hg/store/packs/114243ab99c9697960e23423df9d98f259a0159d.histpack
  .hg/store/packs/6fdfb86776d8d88be3b98aa72ae1c06ca54765f2.dataidx
  .hg/store/packs/6fdfb86776d8d88be3b98aa72ae1c06ca54765f2.datapack

# check that loose files have been removed
  $ findfilessorted .hg/store/data

# repacking all
  $ hg repack
  $ findfilessorted .hg/store/packs
  .hg/store/packs/114243ab99c9697960e23423df9d98f259a0159d.histidx
  .hg/store/packs/114243ab99c9697960e23423df9d98f259a0159d.histpack
  .hg/store/packs/6fdfb86776d8d88be3b98aa72ae1c06ca54765f2.dataidx
  .hg/store/packs/6fdfb86776d8d88be3b98aa72ae1c06ca54765f2.datapack

# incremental repacking with a maxpacksize setting doesn't delete local data even if the pack files are large
  $ hg repack --incremental --debug --config packs.maxpacksize=1
  removing oversize packfile $TESTTMP/hgcache/master/packs/2021b67b6df3cec03f6ca46b83a3e69a67b204ec.datapack (130 bytes)
  removing oversize packfile $TESTTMP/hgcache/master/packs/2021b67b6df3cec03f6ca46b83a3e69a67b204ec.dataidx (1.05 KB)

# check the commit data
  $ hg cat -r . new_file
  new commit

# Verify that temporary datapacks are removed
  $ touch -a -t 201901010000 $TESTTMP/hgcache/master/packs/foo.datapack-tmp
  $ find $CACHEDIR -type f | sort
  $TESTTMP/hgcache/master/packs/28c4bd4f9174a2c5f750bd5612d53e3212e0aaf9.histidx
  $TESTTMP/hgcache/master/packs/28c4bd4f9174a2c5f750bd5612d53e3212e0aaf9.histpack
  $TESTTMP/hgcache/master/packs/foo.datapack-tmp
  $TESTTMP/hgcache/master/packs/repacklock
  $ hg repack
  $ find $CACHEDIR -type f | sort
  $TESTTMP/hgcache/master/packs/28c4bd4f9174a2c5f750bd5612d53e3212e0aaf9.histidx
  $TESTTMP/hgcache/master/packs/28c4bd4f9174a2c5f750bd5612d53e3212e0aaf9.histpack
  $TESTTMP/hgcache/master/packs/repacklock
