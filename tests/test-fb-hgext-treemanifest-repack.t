  $ enable amend
  $ setconfig extensions.treemanifest=!
  $ . "$TESTDIR/library.sh"
  $ setconfig treemanifest.treeonly=False


  $ hg init master
  $ hg clone -q ssh://user@dummy/master client

  $ cd master
  $ echo a > a && hg commit -Aqm 'add a'
  $ mkdir dir && echo b > dir/b && hg commit -Aqm 'add dir/b'

  $ cd ../client
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > fastmanifest=
  > treemanifest=
  > 
  > [remotefilelog]
  > reponame=master
  > 
  > [fastmanifest]
  > usetree=True
  > usecache=False
  > 
  > [treemanifest]
  > autocreatetrees=True
  > EOF

# Test repacking shared manifest packs
  $ hg pull -q -r 0
  $ hg pull -q -r 1
  $ ls_l $CACHEDIR/master/packs/manifests | grep pack
  -r--r--r--     256 0369e6459e768b72223a4a4fcbba59b5ada8d08f.datapack
  -r--r--r--      89 4301ce26f4c07686220c7f57d80b466cfba9899e.histpack
  -r--r--r--     104 4ad892ec0ccef1d0cc04e31986a6b7b4f3a5abbf.datapack
  -r--r--r--     180 7da383a74e4ff5333b3733b9a52eb05c40b1df3d.histpack

- Verify datapack contents
  $ for i in $CACHEDIR/master/packs/manifests/*.datapack; do
  >   echo $i
  >   hg debugdatapack "$i"
  > done
  $TESTTMP/hgcache/master/packs/manifests/0369e6459e768b72223a4a4fcbba59b5ada8d08f.datapack
  $TESTTMP/hgcache/master/packs/manifests/0369e6459e768b72223a4a4fcbba59b5ada8d08f:
  dir:
  Node          Delta Base    Delta Length  Blob Size
  23226e7a252c  000000000000  43            (missing)
  
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  1832e0765de9  000000000000  89            (missing)
  
  $TESTTMP/hgcache/master/packs/manifests/4ad892ec0ccef1d0cc04e31986a6b7b4f3a5abbf.datapack
  $TESTTMP/hgcache/master/packs/manifests/4ad892ec0ccef1d0cc04e31986a6b7b4f3a5abbf:
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  a0c8bcbbb45c  000000000000  43            (missing)
  

- Verify histpack contents
  $ for i in $CACHEDIR/master/packs/manifests/*.histpack; do
  >   echo $i
  >   hg debughistorypack "$i"
  > done
  $TESTTMP/hgcache/master/packs/manifests/4301ce26f4c07686220c7f57d80b466cfba9899e.histpack
  
  
  Node          P1 Node       P2 Node       Link Node     Copy From
  a0c8bcbbb45c  000000000000  000000000000  1f0dee641bb7  
  $TESTTMP/hgcache/master/packs/manifests/7da383a74e4ff5333b3733b9a52eb05c40b1df3d.histpack
  
  
  Node          P1 Node       P2 Node       Link Node     Copy From
  1832e0765de9  a0c8bcbbb45c  000000000000  8e83608cbe60  
  
  dir
  Node          P1 Node       P2 Node       Link Node     Copy From
  23226e7a252c  000000000000  000000000000  8e83608cbe60  

- Repack and reverify
  $ hg repack

  $ ls_l $CACHEDIR/master/packs/manifests | grep pack
  -r--r--r--     262 7535b6084226436bbdff33043969e7fa963e8428.histpack
  -r--r--r--     359 9c7208075283331849ac9c036963e5d873ac4075.datapack

  $ hg debugdatapack $CACHEDIR/master/packs/manifests/*.datapack
  $TESTTMP/hgcache/master/packs/manifests/9c7208075283331849ac9c036963e5d873ac4075:
  dir:
  Node          Delta Base    Delta Length  Blob Size
  23226e7a252c  000000000000  43            (missing)
  
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  1832e0765de9  000000000000  89            (missing)
  a0c8bcbbb45c  000000000000  43            (missing)
  

  $ hg debughistorypack $CACHEDIR/master/packs/manifests/*.histpack
  
  
  Node          P1 Node       P2 Node       Link Node     Copy From
  1832e0765de9  a0c8bcbbb45c  000000000000  8e83608cbe60  
  a0c8bcbbb45c  000000000000  000000000000  1f0dee641bb7  
  
  dir
  Node          P1 Node       P2 Node       Link Node     Copy From
  23226e7a252c  000000000000  000000000000  8e83608cbe60  

# Test repacking local manifest packs
  $ hg up -q 1
  $ echo a >> a && hg commit -Aqm 'modify a'
  $ echo b >> dir/b && hg commit -Aqm 'modify dir/b'
  $ ls_l .hg/store/packs/manifests | grep datapack
  -r--r--r--     150 258363c2f85fdf980f6849495909f19336125a9a.datapack
  -r--r--r--     256 500a3e20e412979cb4b719bd2bb41a9a1425a1d8.datapack

# As we only have packs, also test that --packsonly doesn't prevent packs from
being repacked
  $ hg repack
  $ ls_l .hg/store/packs/manifests | grep datapack
  -r--r--r--     405 0a77c75cd1e70648a515be953b8d8fc842ce00f3.datapack

# Test incremental repacking of trees
  $ echo b >> dir/b && hg commit -Aqm 'modify dir/b'
  $ echo b >> dir/b && hg commit -Aqm 'modify dir/b'
  $ ls_l .hg/store/packs/manifests | grep datapack
  -r--r--r--     405 0a77c75cd1e70648a515be953b8d8fc842ce00f3.datapack
  -r--r--r--     256 6a20e3744f7859d46240ff781f90056825af3c0c.datapack
  -r--r--r--     256 c3bb90e5d6d6906fdc1653487ab2f0b055726090.datapack

  $ echo b >> dir/b && hg commit -Aqm 'modify dir/b'
  $ echo b >> dir/b && hg commit -Aqm 'modify dir/b'
  $ echo b >> dir/b && hg commit -Aqm 'modify dir/b'
  $ ls_l .hg/store/packs/manifests | grep datapack
  -r--r--r--     405 0a77c75cd1e70648a515be953b8d8fc842ce00f3.datapack
  -r--r--r--     256 403049b40660b78ebf7c3bdebefc14595ce148e7.datapack
  -r--r--r--     256 68944bb5b7e904191b7527dee071ae36b15096e9.datapack
  -r--r--r--     256 6a20e3744f7859d46240ff781f90056825af3c0c.datapack
  -r--r--r--     256 c3bb90e5d6d6906fdc1653487ab2f0b055726090.datapack
  -r--r--r--     256 ed87e44e45980f08dc560d64c17762f6e22b813a.datapack
  $ cd .hg/store/packs/manifests
  $ for i in *.data*; do
  >   cp $i x$i
  > done
  $ cd ../../../../

- repack incremental kicks in once there are a number of packs
- (set the repacksizelimit so that we test that we only repack up to 2500 bytes,
- and it leaves one datapack behind)
  >>> sorted(__import__('os').listdir('.hg/store/packs/manifests'))
  ['0a77c75cd1e70648a515be953b8d8fc842ce00f3.dataidx', '0a77c75cd1e70648a515be953b8d8fc842ce00f3.datapack', '403049b40660b78ebf7c3bdebefc14595ce148e7.dataidx', '403049b40660b78ebf7c3bdebefc14595ce148e7.datapack', '4ea308dfd16203929fb0bf3680f989a47e149bcb.histidx', '4ea308dfd16203929fb0bf3680f989a47e149bcb.histpack', '68944bb5b7e904191b7527dee071ae36b15096e9.dataidx', '68944bb5b7e904191b7527dee071ae36b15096e9.datapack', '6a20e3744f7859d46240ff781f90056825af3c0c.dataidx', '6a20e3744f7859d46240ff781f90056825af3c0c.datapack', '7600674ba5e72a96a6ffe14eaccfe7be22f3ed4b.histidx', '7600674ba5e72a96a6ffe14eaccfe7be22f3ed4b.histpack', '7d8575ec97a220b0502a708d4e50e529e2d4c078.histidx', '7d8575ec97a220b0502a708d4e50e529e2d4c078.histpack', '97f06956e87f262fdf5e012456bc0f8bf4d419d6.histidx', '97f06956e87f262fdf5e012456bc0f8bf4d419d6.histpack', 'c3bb90e5d6d6906fdc1653487ab2f0b055726090.dataidx', 'c3bb90e5d6d6906fdc1653487ab2f0b055726090.datapack', 'd734760d7080518750728e2790a48380e4ae0d1b.histidx', 'd734760d7080518750728e2790a48380e4ae0d1b.histpack', 'ed87e44e45980f08dc560d64c17762f6e22b813a.dataidx', 'ed87e44e45980f08dc560d64c17762f6e22b813a.datapack', 'fc3731035492274d99bf46aabb3ed39b908e18fa.histidx', 'fc3731035492274d99bf46aabb3ed39b908e18fa.histpack', 'x0a77c75cd1e70648a515be953b8d8fc842ce00f3.dataidx', 'x0a77c75cd1e70648a515be953b8d8fc842ce00f3.datapack', 'x403049b40660b78ebf7c3bdebefc14595ce148e7.dataidx', 'x403049b40660b78ebf7c3bdebefc14595ce148e7.datapack', 'x68944bb5b7e904191b7527dee071ae36b15096e9.dataidx', 'x68944bb5b7e904191b7527dee071ae36b15096e9.datapack', 'x6a20e3744f7859d46240ff781f90056825af3c0c.dataidx', 'x6a20e3744f7859d46240ff781f90056825af3c0c.datapack', 'xc3bb90e5d6d6906fdc1653487ab2f0b055726090.dataidx', 'xc3bb90e5d6d6906fdc1653487ab2f0b055726090.datapack', 'xed87e44e45980f08dc560d64c17762f6e22b813a.dataidx', 'xed87e44e45980f08dc560d64c17762f6e22b813a.datapack']
  $ hg repack --incremental --config remotefilelog.data.generations=300,200 --config remotefilelog.data.repacksizelimit=2500B
  >>> sorted(__import__('os').listdir('.hg/store/packs/manifests'))
  ['5e242ae58cd6475a97b6278ad1a56ee1937a3f40.histidx', '5e242ae58cd6475a97b6278ad1a56ee1937a3f40.histpack', 'eab1ab4c0b2e59777fd22ab0e5f009ae3f5cd149.dataidx', 'eab1ab4c0b2e59777fd22ab0e5f009ae3f5cd149.datapack']
  $ ls_l .hg/store/packs/manifests | grep datapack | grep 256
  [1]

- Clean up the pile of packs we made
  $ hg repack
