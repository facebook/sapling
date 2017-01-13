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
  $ ls -l $CACHEDIR/master/packs/manifests
  total * (glob)
  * 1146 * 194862a96c9896c52b5dbc5502998d76501edb2e.dataidx (glob)
  *  316 * 194862a96c9896c52b5dbc5502998d76501edb2e.datapack (glob)

  $ hg debugdatapack $CACHEDIR/master/packs/manifests/194862a96c9896c52b5dbc5502998d76501edb2e
  
  
  Node          Delta Base    Delta Length
  bc0c2c938b92  000000000000  43
  
  subdir/
  Node          Delta Base    Delta Length
  ddb35f099a64  000000000000  43
  
  
  Node          Delta Base    Delta Length
  70f2c6726cec  bc0c2c938b92  61

Test that commit creates local trees
  $ hg up -q tip
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ echo z >> subdir/z
  $ hg commit -qAm 'modify subdir/z'
  $ ls -l .hg/store/packs/manifests
  * (glob)
  * 1106 * 1dd1aee1963db4b1c6cd63c0d5a9cbec352481a9.dataidx (glob)
  * 230 * 1dd1aee1963db4b1c6cd63c0d5a9cbec352481a9.datapack (glob)
  $ hg debugdatapack .hg/store/packs/manifests/1dd1aee1963db4b1c6cd63c0d5a9cbec352481a9
  
  subdir/
  Node          Delta Base    Delta Length
  ac728a786423  ddb35f099a64  55
  
  
  Node          Delta Base    Delta Length
  7a911436836f  70f2c6726cec  61

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
