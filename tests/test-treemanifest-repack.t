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
  *  180 * 49bfd5c81a3ca40372ddaea09c4c23c40934a198.histpack (glob)
  *   89 * 54d5b52963363915130d0d7bcddcfd70be1dd0fc.histpack (glob)
  *  100 * 65df85879cdd898607ee3f323a0b61edc7de25b8.datapack (glob)
  *  248 * bb55d9105672c45d4f82df15bd091a555ef02c79.datapack (glob)

- Verify datapack contents
  $ hg debugdatapack $CACHEDIR/master/packs/manifests/65df85879cdd898607ee3f323a0b61edc7de25b8
  
  
  Node          Delta Base    Delta Length
  a0c8bcbbb45c  000000000000  43

  $ hg debugdatapack $CACHEDIR/master/packs/manifests/bb55d9105672c45d4f82df15bd091a555ef02c79
  
  dir
  Node          Delta Base    Delta Length
  23226e7a252c  000000000000  43
  
  
  Node          Delta Base    Delta Length
  1832e0765de9  000000000000  89

- Verify datapack contents
  $ hg debughistorypack $CACHEDIR/master/packs/manifests/49bfd5c81a3ca40372ddaea09c4c23c40934a198
  
  
  Node          P1 Node       P2 Node       Link Node     Copy From
  1832e0765de9  a0c8bcbbb45c  000000000000  8e83608cbe60  
  
  dir
  Node          P1 Node       P2 Node       Link Node     Copy From
  23226e7a252c  000000000000  000000000000  8e83608cbe60  

  $ hg debughistorypack $CACHEDIR/master/packs/manifests/54d5b52963363915130d0d7bcddcfd70be1dd0fc
  
  
  Node          P1 Node       P2 Node       Link Node     Copy From
  a0c8bcbbb45c  000000000000  000000000000  1f0dee641bb7  

- Repack and reverify
  $ hg repack

  $ ls -l $CACHEDIR/master/packs/manifests | grep pack
  *  262 * 6ef9454b3616ff75edca21af6f617d21a79f5963.histpack (glob)
  *  315 * d24c358c968883e3b6c4bd6a85845dfb28fd8de6.datapack (glob)

  $ hg debugdatapack $CACHEDIR/master/packs/manifests/d24c358c968883e3b6c4bd6a85845dfb28fd8de6
  
  
  Node          Delta Base    Delta Length
  1832e0765de9  000000000000  89
  a0c8bcbbb45c  1832e0765de9  12
  
  dir
  Node          Delta Base    Delta Length
  23226e7a252c  000000000000  43

  $ hg debughistorypack $CACHEDIR/master/packs/manifests/6ef9454b3616ff75edca21af6f617d21a79f5963
  
  
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
  $ ls -l .hg/store/packs/manifests | grep datapack
  * 248 * 5d1716bbef6e7200192de6509055d1ee31a4172c.datapack (glob)
  * 146 * cffef142da32f3e52c1779490e5d0ddac5f9b82b.datapack (glob)
  $ hg repack
  $ ls -l .hg/store/packs/manifests | grep datapack
  * 393 * 45f06dbb5d82e52ae3975af82d7e1b4d73c8c599.datapack (glob)

# Test incremental repacking of trees
  $ echo b >> dir/b && hg commit -Aqm 'modify dir/b'
  $ echo b >> dir/b && hg commit -Aqm 'modify dir/b'
  $ ls -l .hg/store/packs/manifests | grep datapack
  * 248 * 21501384df03b8489b366c5218be639fa08830e4.datapack (glob)
  * 393 * 45f06dbb5d82e52ae3975af82d7e1b4d73c8c599.datapack (glob)
  * 248 * d7e689a91ac63385be120a118af9ce8663748f28.datapack (glob)

- repack incremental does nothing here because there are so few packs
  $ hg repack --incremental --config remotefilelog.data.generations=300,200 --config remotefilelog.data.repacksizelimit=300
  $ ls -l .hg/store/packs/manifests | grep datapack
  * 248 * 21501384df03b8489b366c5218be639fa08830e4.datapack (glob)
  * 393 * 45f06dbb5d82e52ae3975af82d7e1b4d73c8c599.datapack (glob)
  * 248 * d7e689a91ac63385be120a118af9ce8663748f28.datapack (glob)

  $ echo b >> dir/b && hg commit -Aqm 'modify dir/b'
  $ echo b >> dir/b && hg commit -Aqm 'modify dir/b'
  $ echo b >> dir/b && hg commit -Aqm 'modify dir/b'
  $ ls -l .hg/store/packs/manifests | grep datapack
  * 248 * 21501384df03b8489b366c5218be639fa08830e4.datapack (glob)
  * 248 * 347263bf1efbdb5bf7e1d1565b6b504073fb9093.datapack (glob)
  * 393 * 45f06dbb5d82e52ae3975af82d7e1b4d73c8c599.datapack (glob)
  * 248 * 544a3b46a61732209116ae50847ec333b75e3765.datapack (glob)
  * 248 * 863908ef8149261ab0d891c2344d8e8766c39441.datapack (glob)
  * 248 * d7e689a91ac63385be120a118af9ce8663748f28.datapack (glob)

- repack incremental kicks in once there are a number of packs
  $ hg repack --incremental --config remotefilelog.data.generations=300,200
  $ ls -l .hg/store/packs/manifests | grep datapack
  *  393 * 45f06dbb5d82e52ae3975af82d7e1b4d73c8c599.datapack (glob)
  * 1236 * 59f37d77ac8d5da86d6eea390010d0d46d9dae19.datapack (glob)

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

  $ hg repack
  $ ls .hg/cache/packs/manifests
  6ef9454b3616ff75edca21af6f617d21a79f5963.histidx
  6ef9454b3616ff75edca21af6f617d21a79f5963.histpack
  d24c358c968883e3b6c4bd6a85845dfb28fd8de6.dataidx
  d24c358c968883e3b6c4bd6a85845dfb28fd8de6.datapack
  $ hg debugdatapack .hg/cache/packs/manifests/*.datapack
  
  
  Node          Delta Base    Delta Length
  1832e0765de9  000000000000  89
  a0c8bcbbb45c  1832e0765de9  12
  
  dir
  Node          Delta Base    Delta Length
  23226e7a252c  000000000000  43
