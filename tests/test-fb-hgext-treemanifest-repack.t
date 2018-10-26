  $ . "$TESTDIR/library.sh"


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
  > usefastdatapack=True
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
  -r--r--r--      89 4301ce26f4c07686220c7f57d80b466cfba9899e.histpack
  -r--r--r--     100 65df85879cdd898607ee3f323a0b61edc7de25b8.datapack
  -r--r--r--     180 7da383a74e4ff5333b3733b9a52eb05c40b1df3d.histpack
  -r--r--r--     248 bb55d9105672c45d4f82df15bd091a555ef02c79.datapack

- Verify datapack contents
  $ for i in $CACHEDIR/master/packs/manifests/*.datapack; do
  >   echo $i
  >   hg debugdatapack "$i"
  > done
  $TESTTMP/hgcache/master/packs/manifests/65df85879cdd898607ee3f323a0b61edc7de25b8.datapack
  $TESTTMP/hgcache/master/packs/manifests/65df85879cdd898607ee3f323a0b61edc7de25b8:
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  a0c8bcbbb45c  000000000000  43            (missing)
  
  $TESTTMP/hgcache/master/packs/manifests/bb55d9105672c45d4f82df15bd091a555ef02c79.datapack
  $TESTTMP/hgcache/master/packs/manifests/bb55d9105672c45d4f82df15bd091a555ef02c79:
  dir:
  Node          Delta Base    Delta Length  Blob Size
  23226e7a252c  000000000000  43            (missing)
  
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  1832e0765de9  000000000000  89            (missing)
  

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
  -r--r--r--     339 56e8c6f0ca2a324b8b5ca1a2730323a1b4d0793a.datapack
  -r--r--r--     262 7535b6084226436bbdff33043969e7fa963e8428.histpack

  $ hg debugdatapack $CACHEDIR/master/packs/manifests/*.datapack
  $TESTTMP/hgcache/master/packs/manifests/56e8c6f0ca2a324b8b5ca1a2730323a1b4d0793a:
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  1832e0765de9  000000000000  89            89
  a0c8bcbbb45c  1832e0765de9  12            43
  
  Total:                      101           132       (23.5% smaller)
  dir:
  Node          Delta Base    Delta Length  Blob Size
  23226e7a252c  000000000000  43            43
  
  Total:                      43            43        (0.0% bigger)

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
  -r--r--r--     248 5d1716bbef6e7200192de6509055d1ee31a4172c.datapack
  -r--r--r--     146 cffef142da32f3e52c1779490e5d0ddac5f9b82b.datapack

# As we only have packs, also test that --packsonly doesn't prevent packs from
being repacked
  $ hg repack --packsonly
  $ ls_l .hg/store/packs/manifests | grep datapack
  -r--r--r--     386 d15c09a9a5a13bb689bd9764455a415a20dc885e.datapack

# Test incremental repacking of trees
  $ echo b >> dir/b && hg commit -Aqm 'modify dir/b'
  $ echo b >> dir/b && hg commit -Aqm 'modify dir/b'
  $ ls_l .hg/store/packs/manifests | grep datapack
  -r--r--r--     248 21501384df03b8489b366c5218be639fa08830e4.datapack
  -r--r--r--     386 d15c09a9a5a13bb689bd9764455a415a20dc885e.datapack
  -r--r--r--     248 d7e689a91ac63385be120a118af9ce8663748f28.datapack

- repack incremental does nothing here because there are so few packs
  $ hg repack --incremental --config remotefilelog.data.generations=300,200 --config remotefilelog.data.repacksizelimit=300
  $ ls_l .hg/store/packs/manifests | grep datapack
  -r--r--r--     248 21501384df03b8489b366c5218be639fa08830e4.datapack
  -r--r--r--     386 d15c09a9a5a13bb689bd9764455a415a20dc885e.datapack
  -r--r--r--     248 d7e689a91ac63385be120a118af9ce8663748f28.datapack

  $ echo b >> dir/b && hg commit -Aqm 'modify dir/b'
  $ echo b >> dir/b && hg commit -Aqm 'modify dir/b'
  $ echo b >> dir/b && hg commit -Aqm 'modify dir/b'
  $ ls_l .hg/store/packs/manifests | grep datapack
  -r--r--r--     248 21501384df03b8489b366c5218be639fa08830e4.datapack
  -r--r--r--     248 347263bf1efbdb5bf7e1d1565b6b504073fb9093.datapack
  -r--r--r--     248 544a3b46a61732209116ae50847ec333b75e3765.datapack
  -r--r--r--     248 863908ef8149261ab0d891c2344d8e8766c39441.datapack
  -r--r--r--     386 d15c09a9a5a13bb689bd9764455a415a20dc885e.datapack
  -r--r--r--     248 d7e689a91ac63385be120a118af9ce8663748f28.datapack
  $ cd .hg/store/packs/manifests
  $ cp d7e689a91ac63385be120a118af9ce8663748f28.datapack x7e689a91ac63385be120a118af9ce8663748f28.datapack
  $ cp d7e689a91ac63385be120a118af9ce8663748f28.dataidx x7e689a91ac63385be120a118af9ce8663748f28.dataidx
  $ cp 21501384df03b8489b366c5218be639fa08830e4.datapack x1501384df03b8489b366c5218be639fa08830e4.datapack
  $ cp 21501384df03b8489b366c5218be639fa08830e4.dataidx x1501384df03b8489b366c5218be639fa08830e4.dataidx
  $ cp 347263bf1efbdb5bf7e1d1565b6b504073fb9093.datapack x47263bf1efbdb5bf7e1d1565b6b504073fb9093.datapack
  $ cp 347263bf1efbdb5bf7e1d1565b6b504073fb9093.dataidx x47263bf1efbdb5bf7e1d1565b6b504073fb9093.dataidx
  $ cd ../../../../

- repack incremental kicks in once there are a number of packs
- (set the repacksizelimit so that we test that we only repack up to 1500 bytes,
- and it leaves one datapack behind)
  $ hg repack --incremental --config remotefilelog.data.generations=300,200 --config remotefilelog.data.repacksizelimit=1500B
  $ ls_l .hg/store/packs/manifests | grep datapack | wc -l
  .*3 (re)
  $ ls_l .hg/store/packs/manifests | grep datapack | grep 248
  -r--r--r--     248 *.datapack (glob)

- incremental repacking with a maxpacksize setting doesn't delete local data even if the pack files are large
  $ hg repack --incremental --debug --config packs.maxpacksize=1
  removing oversize packfile $TESTTMP/hgcache/master/packs/manifests/56e8c6f0ca2a324b8b5ca1a2730323a1b4d0793a.datapack (339 bytes)
  removing oversize packfile $TESTTMP/hgcache/master/packs/manifests/56e8c6f0ca2a324b8b5ca1a2730323a1b4d0793a.dataidx (1.13 KB)

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

- Test pruning the manifest cache using packs.maxpackfilecount
  $ hg repack --incremental --config packs.maxpackfilecount=0
  $ hg repack --incremental --config packs.maxpackfilecount=1
  purging shared treemanifest pack cache (4 entries) -- too many files
  $ ls_l .hg/cache/packs/manifests/
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
