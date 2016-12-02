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
  *  214 * ed42c8e98d598b7c9de7c2660f2a833bb5198b54.datapack (glob)

  $ hg debugdatapack $CACHEDIR/master/packs/manifests/65df85879cdd898607ee3f323a0b61edc7de25b8
  
  
  Node          Delta Base    Delta Length
  a0c8bcbbb45c  000000000000  43

  $ hg debugdatapack $CACHEDIR/master/packs/manifests/ed42c8e98d598b7c9de7c2660f2a833bb5198b54
  
  dir/
  Node          Delta Base    Delta Length
  23226e7a252c  000000000000  43
  
  
  Node          Delta Base    Delta Length
  1832e0765de9  a0c8bcbbb45c  58
  $ hg repack

  $ ls -l $CACHEDIR/master/packs/manifests | grep datapack
  *  313 * c217b22cf43133a289290b6ac32d95f2b5a8361e.datapack (glob)

  $ hg debugdatapack $CACHEDIR/master/packs/manifests/c217b22cf43133a289290b6ac32d95f2b5a8361e
  
  
  Node          Delta Base    Delta Length
  1832e0765de9  a0c8bcbbb45c  58
  a0c8bcbbb45c  000000000000  43
  
  dir/
  Node          Delta Base    Delta Length
  23226e7a252c  000000000000  43

# Test repacking local manifest packs
  $ hg up -q 1
  $ echo a >> a && hg commit -Aqm 'modify a'
  $ echo b >> dir/b && hg commit -Aqm 'modify dir/b'
  $ ls -l .hg/store/packs/manifests | grep datapack
  * 109 * 4465e7e50fbf4559eb4df204edd9be788cc346a5.datapack (glob)
  * 227 * f1c10c3d58e94f19ec2978407ead3dba42558419.datapack (glob)
  $ hg repack
  $ ls -l .hg/store/packs/manifests | grep datapack
  * 335 * 3c6e0e5aee5fbadb6c70cab831e9ec4921e5d99c.datapack (glob)
