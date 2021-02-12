#chg-compatible

  $ disable treemanifest
  $ . "$TESTDIR/library.sh"
  $ setconfig remotefilelog.write-hgcache-to-indexedlog=False remotefilelog.write-local-to-indexedlog=False

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
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob) (?)

  $ cd shallow
  $ find $CACHEDIR/master/packs | sort
  $TESTTMP/hgcache/master/packs
  $TESTTMP/hgcache/master/packs/276d308429d0303762befa376788300f0310f90e.histidx
  $TESTTMP/hgcache/master/packs/276d308429d0303762befa376788300f0310f90e.histpack
  $TESTTMP/hgcache/master/packs/887690f1138ae5b99c50d754ed02262874bf8ecb.dataidx
  $TESTTMP/hgcache/master/packs/887690f1138ae5b99c50d754ed02262874bf8ecb.datapack

  $ touch -m -t 200001010000 $TESTTMP/hgcache/master/packs/887690f1138ae5b99c50d754ed02262874bf8ecb.datapack

# Cleanup the old over the limit packfiles
  $ hg repack --config remotefilelog.cleanoldpacks=True --config remotefilelog.cachelimit="10B"

  $ find $CACHEDIR/master/packs | sort
  $TESTTMP/hgcache/master/packs
  $TESTTMP/hgcache/master/packs/276d308429d0303762befa376788300f0310f90e.histidx
  $TESTTMP/hgcache/master/packs/276d308429d0303762befa376788300f0310f90e.histpack
  $TESTTMP/hgcache/master/packs/repacklock


Cleanup old packs during writes when we're over the threshold

  $ cd ../master
  $ echo 12345678901234567890123456789012345678901234567890 > a
  $ echo 12345678901234567890123456789012345678901234567890 > b
  $ echo 12345678901234567890123456789012345678901234567890 > c
  $ echo 12345678901234567890123456789012345678901234567890 > d
  $ echo 12345678901234567890123456789012345678901234567890 > e
  $ hg commit -Aqm "add a bunch of files"
  $ cd ../shallow
  $ hg pull -q
  $ clearcache
  $ hg up -q tip --config packs.maxdatapendingbytes=30
  $ ls_l $CACHEDIR/master/packs | grep datapack | sort
  -r--r--r--     144 *.datapack (glob)
  -r--r--r--      80 *.datapack (glob)
  -r--r--r--      80 *.datapack (glob)
  -r--r--r--      80 *.datapack (glob)
  -r--r--r--      80 *.datapack (glob)
  $ hg up -q null

  $ clearcache
  $ hg up -q tip --config packs.maxdatapendingbytes=30 --config packs.maxdatabytes=120
  $ ls_l $CACHEDIR/master/packs | grep datapack | sort
  -r--r--r--      65 *.datapack (glob)
  -r--r--r--      80 *.datapack (glob)
  $ hg up -q null

  $ clearcache
  $ hg up -q tip --config packs.maxdatapendingbytes=30 --config packs.maxdatabytes=200
  $ ls_l $CACHEDIR/master/packs | grep datapack | sort
  -r--r--r--      65 *.datapack (glob)
  -r--r--r--      80 *.datapack (glob)
  -r--r--r--      80 *.datapack (glob)
