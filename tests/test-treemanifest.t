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
  > fastmanifest=$TESTDIR/../fastmanifest
  > treemanifest=$TESTDIR/../treemanifest
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
  -r--r--r--    1224 ed1a27864c5d25f144a51961ad6e79088f2a7571.histidx
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
  -r--r--r--    1196 57710544ca24ac4f36682ec279959879c92a3275.histidx
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

Test rebasing a stack of commits results in a pack with all the trees

  $ echo >> subdir/y
  $ hg commit -qAm 'modify subdir/y'
  $ echo >> subdir/y
  $ hg commit -Am 'modify subdir/y again'
  $ hg rebase -d 0 -s '.^'
  rebasing 3:6a2476258ba5 "modify subdir/y"
  rebasing 4:f096b21e165f "modify subdir/y again" (tip)
  saved backup bundle to $TESTTMP/client/.hg/strip-backup/6a2476258ba5-a90056a1-rebase.hg (glob)
  $ hg log -r '.^::.' -T '{manifest}\n'
  3:0d05c20bb7eb
  4:8289b85c6a30
  $ hg debugdatapack .hg/store/packs/manifests/5a5fb42e99986c90ac86b57d184561c44238b7b7.datapack
  
  subdir
  Node          Delta Base    Delta Length
  a4e2f032ee0f  000000000000  43
  
  
  Node          Delta Base    Delta Length
  0d05c20bb7eb  000000000000  92
  
  subdir
  Node          Delta Base    Delta Length
  ad0a48a2ec1e  000000000000  43
  
  
  Node          Delta Base    Delta Length
  8289b85c6a30  000000000000  92

Test treemanifest with sparse enabled
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > sparse = $TESTDIR/../hgext3rd/sparse.py
  > reset = $TESTDIR/../hgext3rd/reset.py
  > EOF
  $ hg sparse -I subdir
  $ hg reset '.^'
  resetting without an active bookmark
  saved backup bundle to $TESTTMP/client/.hg/strip-backup/27a577922312-3ad85b1a-backup.hg (glob)
  $ hg status
  M subdir/y
  $ hg up -C .
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg sparse --reset

Test rebase two commits with same changes
  $ echo >> subdir/y
  $ hg commit -qm 'modify subdir/y #1'
  $ hg up -q '.^'
  $ echo >> x
  $ echo >> subdir/y
  $ hg commit -qm 'modify subdir/y #2'
  $ hg up -q '.^'
  $ echo >> noop
  $ hg add noop
  $ hg commit -Am 'rebase destination'
  created new head
  $ hg rebase -d 6 -s '4 + 5' --config rebase.singletransaction=True
  rebasing 4:6052526a0d67 "modify subdir/y #1"
  rebasing 5:79a69a1547d7 "modify subdir/y #2"
  saved backup bundle to $TESTTMP/client/.hg/strip-backup/79a69a1547d7-fc6bc129-rebase.hg (glob)
  $ hg debughistorypack .hg/store/packs/manifests/668716b2b93e2fa8151bca1a327a391746f0dcc0.histpack
  
  
  Node          P1 Node       P2 Node       Link Node     Copy From
  8026e03c5a35  8011431de863  000000000000  000000000000  
  5ca06dca517c  8011431de863  000000000000  000000000000  
  
  subdir
  Node          P1 Node       P2 Node       Link Node     Copy From
  ad0a48a2ec1e  a4e2f032ee0f  000000000000  000000000000  
