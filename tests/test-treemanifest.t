  $ . "$TESTDIR/library.sh"

  $ PYTHONPATH=$TESTDIR/..:$PYTHONPATH
  $ export PYTHONPATH

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > server=True
  > EOF
  $ echo x > x
  $ hg commit -qAm 'add x'
  $ cd ..

  $ hgcloneshallow ssh://user@dummy/master client -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ cd client
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > fastmanifest=
  > treemanifest=
  > 
  > [remotefilelog]
  > usefastdatapack=True
  > 
  > [fastmanifest]
  > usetree=True
  > usecache=False
  > 
  > [treemanifest]
  > usecunionstore=True
  > EOF

Test autocreatetrees
  $ cat >> .hg/hgrc <<EOF
  > [treemanifest]
  > autocreatetrees=True
  > EOF
  $ cd ../master
  $ mkdir subdir
  $ echo z >> subdir/z
  $ hg commit -qAm 'add subdir/z'

  $ cd ../client
  $ hg pull
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  (run 'hg update' to get a working copy)
  $ ls_l $CACHEDIR/master/packs/manifests
  -r--r--r--    1146 678f597a73b2b96f2e120c84ef8a84069a250266.dataidx
  -r--r--r--     315 678f597a73b2b96f2e120c84ef8a84069a250266.datapack
  -r--r--r--    1106 ed1a27864c5d25f144a51961ad6e79088f2a7571.histidx
  -r--r--r--     265 ed1a27864c5d25f144a51961ad6e79088f2a7571.histpack

  $ hg debugdatapack $CACHEDIR/master/packs/manifests/678f597a73b2b96f2e120c84ef8a84069a250266
  
  
  Node          Delta Base    Delta Length
  bc0c2c938b92  000000000000  43
  
  subdir
  Node          Delta Base    Delta Length
  ddb35f099a64  000000000000  43
  
  
  Node          Delta Base    Delta Length
  70f2c6726cec  bc0c2c938b92  61

Test that commit creates local trees
  $ hg up -q tip
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ echo z >> subdir/z
  $ hg commit -qAm 'modify subdir/z'
  $ ls_l .hg/store/packs/manifests
  -r--r--r--    1106 57710544ca24ac4f36682ec279959879c92a3275.histidx
  -r--r--r--     183 57710544ca24ac4f36682ec279959879c92a3275.histpack
  -r--r--r--    1106 a7f7e084adff88a01cf76909345be1e56ee704a9.dataidx
  -r--r--r--     254 a7f7e084adff88a01cf76909345be1e56ee704a9.datapack
  $ hg debugdatapack .hg/store/packs/manifests/a7f7e084adff88a01cf76909345be1e56ee704a9
  
  subdir
  Node          Delta Base    Delta Length
  ac728a786423  000000000000  43
  
  
  Node          Delta Base    Delta Length
  7a911436836f  000000000000  92

Test that manifest matchers work
  $ hg status --rev 1 --rev 2 -I subdir/a
  $ hg status --rev 1 --rev 2 -I subdir/z
  M subdir/z

Test config validation
  $ hg log -r . --config extensions.fastmanifest=!
  abort: cannot use treemanifest without fastmanifest
  [255]
  $ hg log -r . --config extensions.treemanifest=!
  abort: fastmanifest.usetree cannot be enabled without enabling treemanifest
  [255]
