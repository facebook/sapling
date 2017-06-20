  $ . "$TESTDIR/library.sh"

  $ PYTHONPATH=$TESTDIR/..:$PYTHONPATH
  $ export PYTHONPATH

  $ hg init master
  $ hg clone -q ssh://user@dummy/master client

  $ cd master
  $ echo a > a && hg commit -Aqm 'add a'
  $ mkdir dir && echo b > dir/b && hg commit -Aqm 'add dir/b'

  $ cd ../client
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > fastmanifest=$TESTDIR/../fastmanifest
  > treemanifest=$TESTDIR/../treemanifest
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
  
  
  Node          Delta Base    Delta Length
  a0c8bcbbb45c  000000000000  43
  $TESTTMP/hgcache/master/packs/manifests/bb55d9105672c45d4f82df15bd091a555ef02c79.datapack
  
  dir
  Node          Delta Base    Delta Length
  23226e7a252c  000000000000  43
  
  
  Node          Delta Base    Delta Length
  1832e0765de9  000000000000  89

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
  -r--r--r--     315 d24c358c968883e3b6c4bd6a85845dfb28fd8de6.datapack

  $ hg debugdatapack $CACHEDIR/master/packs/manifests/*.datapack
  
  
  Node          Delta Base    Delta Length
  1832e0765de9  000000000000  89
  a0c8bcbbb45c  1832e0765de9  12
  
  dir
  Node          Delta Base    Delta Length
  23226e7a252c  000000000000  43

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
  $ hg repack
  $ ls -l .hg/store/packs/manifests | grep datapack
  * 362 * 9e5a04938d53f5b418e4f1b6a413d250f231f60a.datapack (glob)

# Test incremental repacking of trees
  $ echo b >> dir/b && hg commit -Aqm 'modify dir/b'
  $ echo b >> dir/b && hg commit -Aqm 'modify dir/b'
  $ ls -l .hg/store/packs/manifests | grep datapack
  * 248 * 21501384df03b8489b366c5218be639fa08830e4.datapack (glob)
  * 362 * 9e5a04938d53f5b418e4f1b6a413d250f231f60a.datapack (glob)
  * 248 * d7e689a91ac63385be120a118af9ce8663748f28.datapack (glob)

- repack incremental does nothing here because there are so few packs
  $ hg repack --incremental --config remotefilelog.data.generations=300,200 --config remotefilelog.data.repacksizelimit=300
  $ ls -l .hg/store/packs/manifests | grep datapack
  * 248 * 21501384df03b8489b366c5218be639fa08830e4.datapack (glob)
  * 362 * 9e5a04938d53f5b418e4f1b6a413d250f231f60a.datapack (glob)
  * 248 * d7e689a91ac63385be120a118af9ce8663748f28.datapack (glob)

  $ echo b >> dir/b && hg commit -Aqm 'modify dir/b'
  $ echo b >> dir/b && hg commit -Aqm 'modify dir/b'
  $ echo b >> dir/b && hg commit -Aqm 'modify dir/b'
  $ ls -l .hg/store/packs/manifests | grep datapack
  * 248 * 21501384df03b8489b366c5218be639fa08830e4.datapack (glob)
  * 248 * 347263bf1efbdb5bf7e1d1565b6b504073fb9093.datapack (glob)
  * 248 * 544a3b46a61732209116ae50847ec333b75e3765.datapack (glob)
  * 248 * 863908ef8149261ab0d891c2344d8e8766c39441.datapack (glob)
  * 362 * 9e5a04938d53f5b418e4f1b6a413d250f231f60a.datapack (glob)
  * 248 * d7e689a91ac63385be120a118af9ce8663748f28.datapack (glob)

- repack incremental kicks in once there are a number of packs
  $ hg repack --incremental --config remotefilelog.data.generations=300,200
  $ ls -l .hg/store/packs/manifests | grep datapack
  * 1148 * 57360ff79b595e6474abacd912600d61b5e5c840.datapack (glob)
  *  362 * 9e5a04938d53f5b418e4f1b6a413d250f231f60a.datapack (glob)

Test repacking from revlogs to pack files on the server
  $ cd ../master

  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=$TESTDIR/../treemanifest
  > remotefilelog=$TESTDIR/../remotefilelog
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

  $ hg repack
  $ ls .hg/cache/packs/manifests
  7535b6084226436bbdff33043969e7fa963e8428.histidx
  7535b6084226436bbdff33043969e7fa963e8428.histpack
  d24c358c968883e3b6c4bd6a85845dfb28fd8de6.dataidx
  d24c358c968883e3b6c4bd6a85845dfb28fd8de6.datapack
  $ hg debugdatapack .hg/cache/packs/manifests/*.datapack
  
  
  Node          Delta Base    Delta Length
  1832e0765de9  000000000000  89
  a0c8bcbbb45c  1832e0765de9  12
  
  dir
  Node          Delta Base    Delta Length
  23226e7a252c  000000000000  43

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
  
  
  Node          Delta Base    Delta Length
  1832e0765de9  000000000000  89
  
  dir
  Node          Delta Base    Delta Length
  23226e7a252c  000000000000  43
