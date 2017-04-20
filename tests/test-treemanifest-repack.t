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
  $ ls -l $CACHEDIR/master/packs/manifests | grep pack
  *  181 * 095751e2986a69c95ca94f92e02aa5dc9f66570c.histpack (glob)
  *   89 * 54d5b52963363915130d0d7bcddcfd70be1dd0fc.histpack (glob)
  *  100 * 65df85879cdd898607ee3f323a0b61edc7de25b8.datapack (glob)
  *  249 * e61e965008eb4449c7dd33d4cfe650606e00a0c8.datapack (glob)

- Verify datapack contents
  $ hg debugdatapack $CACHEDIR/master/packs/manifests/65df85879cdd898607ee3f323a0b61edc7de25b8
  
  
  Node          Delta Base    Delta Length
  a0c8bcbbb45c  000000000000  43

  $ hg debugdatapack $CACHEDIR/master/packs/manifests/e61e965008eb4449c7dd33d4cfe650606e00a0c8
  
  dir/
  Node          Delta Base    Delta Length
  23226e7a252c  000000000000  43
  
  
  Node          Delta Base    Delta Length
  1832e0765de9  000000000000  89

- Verify datapack contents
  $ hg debughistorypack $CACHEDIR/master/packs/manifests/095751e2986a69c95ca94f92e02aa5dc9f66570c
  
  
  Node          P1 Node       P2 Node       Link Node     Copy From
  1832e0765de9  a0c8bcbbb45c  000000000000  8e83608cbe60  
  
  dir/
  Node          P1 Node       P2 Node       Link Node     Copy From
  23226e7a252c  000000000000  000000000000  8e83608cbe60  

  $ hg debughistorypack $CACHEDIR/master/packs/manifests/54d5b52963363915130d0d7bcddcfd70be1dd0fc
  
  
  Node          P1 Node       P2 Node       Link Node     Copy From
  a0c8bcbbb45c  000000000000  000000000000  1f0dee641bb7  

- Repack and reverify
  $ hg repack

  $ ls -l $CACHEDIR/master/packs/manifests | grep pack
  *  316 * 4fa1c1e1a5f63679cb192ba1ab24f7363a79b7e9.datapack (glob)
  *  263 * 812c2b1b119b5609e5c902abbd99455e32955cdf.histpack (glob)

  $ hg debugdatapack $CACHEDIR/master/packs/manifests/4fa1c1e1a5f63679cb192ba1ab24f7363a79b7e9
  
  
  Node          Delta Base    Delta Length
  1832e0765de9  000000000000  89
  a0c8bcbbb45c  1832e0765de9  12
  
  dir/
  Node          Delta Base    Delta Length
  23226e7a252c  000000000000  43

  $ hg debughistorypack $CACHEDIR/master/packs/manifests/812c2b1b119b5609e5c902abbd99455e32955cdf
  
  
  Node          P1 Node       P2 Node       Link Node     Copy From
  1832e0765de9  a0c8bcbbb45c  000000000000  8e83608cbe60  
  a0c8bcbbb45c  000000000000  000000000000  1f0dee641bb7  
  
  dir/
  Node          P1 Node       P2 Node       Link Node     Copy From
  23226e7a252c  000000000000  000000000000  8e83608cbe60  

# Test repacking local manifest packs
  $ hg up -q 1
  $ echo a >> a && hg commit -Aqm 'modify a'
  $ echo b >> dir/b && hg commit -Aqm 'modify dir/b'
  $ ls -l .hg/store/packs/manifests | grep datapack
  * 249 * 087d03b07be7a9d47094c965deb837846ff58fe1.datapack (glob)
  * 146 * cffef142da32f3e52c1779490e5d0ddac5f9b82b.datapack (glob)
  $ hg repack
  $ ls -l .hg/store/packs/manifests | grep datapack
  * 394 * 6cbda0a4c1d906347a1183153d2e54760d4d6b14.datapack (glob)

# Test incremental repacking of trees
  $ echo b >> dir/b && hg commit -Aqm 'modify dir/b'
  $ echo b >> dir/b && hg commit -Aqm 'modify dir/b'
  $ ls -l .hg/store/packs/manifests | grep datapack
  * 249 * 4ed5d60b25e010b02fd9a79be8bb216e6c43eae7.datapack (glob)
  * 394 * 6cbda0a4c1d906347a1183153d2e54760d4d6b14.datapack (glob)
  * 249 * dfdc5f4d86ae2389fc5660338fb6018fb2000a4b.datapack (glob)

- repack incremental does nothing here because there are so few packs
  $ hg repack --incremental --config remotefilelog.data.generations=300,200 --config remotefilelog.data.repacksizelimit=300
  $ ls -l .hg/store/packs/manifests | grep datapack
  * 249 * 4ed5d60b25e010b02fd9a79be8bb216e6c43eae7.datapack (glob)
  * 394 * 6cbda0a4c1d906347a1183153d2e54760d4d6b14.datapack (glob)
  * 249 * dfdc5f4d86ae2389fc5660338fb6018fb2000a4b.datapack (glob)

  $ echo b >> dir/b && hg commit -Aqm 'modify dir/b'
  $ echo b >> dir/b && hg commit -Aqm 'modify dir/b'
  $ echo b >> dir/b && hg commit -Aqm 'modify dir/b'
  $ ls -l .hg/store/packs/manifests | grep datapack
  * 249 * 4db71afc17447dc1728f4d41d684495ecc073822.datapack (glob)
  * 249 * 4ed5d60b25e010b02fd9a79be8bb216e6c43eae7.datapack (glob)
  * 394 * 6cbda0a4c1d906347a1183153d2e54760d4d6b14.datapack (glob)
  * 249 * 6d64b99736cef7125a5998fa6fbe4c866e146ae7.datapack (glob)
  * 249 * a1750dfd2e97ded5bc62ad05551c9be33ad7de53.datapack (glob)
  * 249 * dfdc5f4d86ae2389fc5660338fb6018fb2000a4b.datapack (glob)

- repack incremental kicks in once there are a number of packs
  $ hg repack --incremental --config remotefilelog.data.generations=300,200
  $ ls -l .hg/store/packs/manifests | grep datapack
  *  394 * 6cbda0a4c1d906347a1183153d2e54760d4d6b14.datapack (glob)
  * 1241 * a55c1202ad93c3a7821490cbe3d7fd9d9030c25a.datapack (glob)
