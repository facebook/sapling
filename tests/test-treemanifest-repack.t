  $ . "$TESTDIR/library.sh"

  $ PYTHONPATH=$TESTDIR/..:$PYTHONPATH
  $ export PYTHONPATH

  $ cat >> $HGRCPATH <<EOF
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
  > EOF

  $ hg init master
  $ hg clone -q master client

  $ cd master
  $ echo a > a && hg commit -Aqm 'add a'
  $ mkdir dir && echo b > dir/b && hg commit -Aqm 'add dir/b'

  $ cd ../client
  $ cat >> .hg/hgrc <<EOF
  > [treemanifest]
  > autocreatetrees=True
  > EOF

# Test repacking shared manifest packs
  $ hg pull -q -r 0
  $ hg pull -q -r 1
  $ ls -l $CACHEDIR/master/packs/manifests | grep datapack
  *  100 * 65df85879cdd898607ee3f323a0b61edc7de25b8.datapack (glob)
  *  249 * e61e965008eb4449c7dd33d4cfe650606e00a0c8.datapack (glob)

  $ hg debugdatapack $CACHEDIR/master/packs/manifests/65df85879cdd898607ee3f323a0b61edc7de25b8
  
  
  Node          Delta Base    Delta Length
  a0c8bcbbb45c  000000000000  43

  $ hg debugdatapack $CACHEDIR/master/packs/manifests/e61e965008eb4449c7dd33d4cfe650606e00a0c8
  
  dir/
  Node          Delta Base    Delta Length
  23226e7a252c  000000000000  43
  
  
  Node          Delta Base    Delta Length
  1832e0765de9  000000000000  89
  $ hg repack

  $ ls -l $CACHEDIR/master/packs/manifests | grep datapack
  *  348 * 8f4e0c3b3331b837667212f806314cbcb2c69f52.datapack (glob)

  $ hg debugdatapack $CACHEDIR/master/packs/manifests/8f4e0c3b3331b837667212f806314cbcb2c69f52
  
  
  Node          Delta Base    Delta Length
  1832e0765de9  000000000000  89
  a0c8bcbbb45c  000000000000  43
  
  dir/
  Node          Delta Base    Delta Length
  23226e7a252c  000000000000  43

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
