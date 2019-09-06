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
  $ hg repack --packsonly
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

- incremental repacking with a maxpacksize setting doesn't delete local data even if the pack files are large
  $ hg repack --incremental --debug --config packs.maxpacksize=1
  removing oversize packfile $TESTTMP/hgcache/master/packs/manifests/9c7208075283331849ac9c036963e5d873ac4075.datapack (359 bytes)
  removing oversize packfile $TESTTMP/hgcache/master/packs/manifests/9c7208075283331849ac9c036963e5d873ac4075.dataidx (1.13 KB)

- Clean up the pile of packs we made
  $ hg repack

Test repacking from revlogs to pack files on the server
  $ cd ../master

  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=
  > remotefilelog=
  > [remotefilelog]
  > server=True
  > [treemanifest]
  > server=True
  > EOF
  $ hg backfilltree
  $ cat .hg/store/fncache | sort
  data/a.i
  data/dir/b.i
  meta/dir/00manifest.i

--packsonly shouldn't repack anything:
  $ hg repack --packsonly
  $ ls .hg/cache/packs/manifests

  $ hg repack
  $ ls .hg/cache/packs/manifests
  56e8c6f0ca2a324b8b5ca1a2730323a1b4d0793a.dataidx
  56e8c6f0ca2a324b8b5ca1a2730323a1b4d0793a.datapack
  7535b6084226436bbdff33043969e7fa963e8428.histidx
  7535b6084226436bbdff33043969e7fa963e8428.histpack
  $ hg debugdatapack .hg/cache/packs/manifests/*.datapack
  .hg/cache/packs/manifests/56e8c6f0ca2a324b8b5ca1a2730323a1b4d0793a:
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  1832e0765de9  000000000000  89            89
  a0c8bcbbb45c  1832e0765de9  12            43
  
  Total:                      101           132       (23.5% smaller)
  dir:
  Node          Delta Base    Delta Length  Blob Size
  23226e7a252c  000000000000  43            43
  
  Total:                      43            43        (0.0% bigger)

Test incremental revlog repacking
# 1. Make commit that we'll need to repack
  $ echo >> a
  $ hg commit -Aqm 'modify a'
  $ hg debugindex .hg/store/00manifesttree.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      44     -1       0 a0c8bcbbb45c 000000000000 000000000000
       1        44      58      0       1 1832e0765de9 a0c8bcbbb45c 000000000000
       2       102      55      1       2 1618a54c483e 1832e0765de9 000000000000

# 2. Corrupt an early rev of the manifesttree, to prove we don't read it
  $ cp .hg/store/00manifesttree.i .hg/store/00manifesttree.i.bak
  $ printf xxxx | dd conv=notrunc of=.hg/store/00manifesttree.i bs=1 seek=32 >/dev/null 2>&1
  $ hg debugindex .hg/store/00manifesttree.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      44     -1       0 78787878b45c 000000000000 000000000000
       1        44      58      0       1 1832e0765de9 78787878b45c 000000000000
       2       102      55      1       2 1618a54c483e 1832e0765de9 000000000000

# 3. Check that the corrupt '78787878...' node is not in the pack
  $ hg repack --incremental
  $ hg debugdatapack .hg/cache/packs/manifests/*.datapack | grep 7878
  [1]
  $ mv .hg/store/00manifesttree.i.bak .hg/store/00manifesttree.i

Test incremental repack with limited revs only repacks those revs
  $ rm -rf .hg/cache/packs/manifests
  $ hg repack --incremental --config treemanifest.repackstartrev=1 --config treemanifest.repackendrev=1
  $ hg debugdatapack .hg/cache/packs/manifests/*.datapack
  .hg/cache/packs/manifests/e9093d2d887ff14457d43338fcb3994e92051853:
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  1832e0765de9  000000000000  89            89
  
  Total:                      89            89        (0.0% bigger)
  dir:
  Node          Delta Base    Delta Length  Blob Size
  23226e7a252c  000000000000  43            43
  
  Total:                      43            43        (0.0% bigger)

Test incremental repack that doesn't take all packs
  $ ls_l .hg/cache/packs/manifests/ | grep datapack
  -r--r--r--     264 e9093d2d887ff14457d43338fcb3994e92051853.datapack

- Only one pack, means don't repack it. Only turn revlogs into a pack.
  $ hg repack --incremental --config remotefilelog.data.generations=300,20
  $ ls_l .hg/cache/packs/manifests/ | grep datapack
  -r--r--r--     264 e9093d2d887ff14457d43338fcb3994e92051853.datapack
  -r--r--r--     154 f9657fdc11d7c9847208da3f1245b38c5981df79.datapack

- Two packs doesn't meet the bar for repack. Only turn revlogs into a pack.
  $ echo >> a
  $ hg commit -m 'modify a'
  $ hg repack --incremental --config remotefilelog.data.generations=300,20
  $ ls_l .hg/cache/packs/manifests/ | grep datapack
  -r--r--r--     154 0adbde90bc92c6f23e46180a9d7885c8e2499173.datapack
  -r--r--r--     264 e9093d2d887ff14457d43338fcb3994e92051853.datapack
  -r--r--r--     154 f9657fdc11d7c9847208da3f1245b38c5981df79.datapack

- Three packs meets the bar. Repack new revlogs and old pack into one.
  $ hg repack --incremental --config remotefilelog.data.generations=300,20
  $ ls_l .hg/cache/packs/manifests/ | grep datapack
  -r--r--r--     496 bc6c2ebb080844d7a227dacbc847a5b375ec620c.datapack

- Test pruning the manifest cache using packs.maxpackfilecount.
- (Use 'hg metaedit' as repack itself will not trigger the purge, and
- 'metaedit' won't create any new objects to pack.)
  $ hg metaedit -m 'modify a (2)' --config packs.maxpackfilecount=0
  $ hg metaedit -m 'modify a (3)' --config packs.maxpackfilecount=1
  purging shared treemanifest pack cache (4 entries) -- too many files
  $ test -d .hg/cache/packs/manifests/
  [1]
  $ cd ..

Test hg gc with multiple repositories
  $ hginit master_remotefilelog_only
  $ cd master_remotefilelog_only
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=
  > [remotefilelog]
  > server=True
  > serverexpiration=-1
  > [treemanifest]
  > server=True
  > EOF
  $ echo x > x
  $ hg commit -qAm x
  $ echo x >> x
  $ hg commit -qAm x2
  $ cd ..

  $ hgcloneshallow ssh://user@dummy/master_remotefilelog_only shallow -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ hgcloneshallow ssh://user@dummy/master_remotefilelog_only shallow_tree -q
  $ cd shallow_tree
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > fastmanifest=
  > treemanifest=
  > [fastmanifest]
  > usetree=True
  > usecache=False
  > EOF
  $ hg gc
  finished: removed 0 of 2 files (0.00 GB to 0.00 GB)
  $ cd ..


  $ hginit master2
  $ cd master2
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=
  > remotefilelog=
  > [remotefilelog]
  > server=True
  > [treemanifest]
  > treeonly=True
  > server=True
  > EOF
  $ echo >> a
  $ hg commit -Aqm 'a'
  $ echo >> b
  $ hg commit -Aqm 'b'
  $ hg debugindex .hg/store/00manifesttree.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      44     -1       0 23dae89e9cb9 000000000000 000000000000
       1        44      55      0       1 b6fc856d0b3b 23dae89e9cb9 000000000000
  $ hg repack --incremental
  $ hg debugdatapack .hg/cache/packs/manifests/e85f1090835eee2f66375fbcbac1be64ee900435.datapack
  .hg/cache/packs/manifests/e85f1090835eee2f66375fbcbac1be64ee900435:
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  b6fc856d0b3b  000000000000  86            86
  
  Total:                      86            86        (0.0% bigger)
