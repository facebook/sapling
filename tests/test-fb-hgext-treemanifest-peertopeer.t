  $ CACHEDIR=`pwd`/hgcache

  $ . "$TESTDIR/library.sh"

Create server
  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=
  > [treemanifest]
  > server=True
  > [remotefilelog]
  > server=True
  > EOF

  $ mkdir subdir
  $ echo x > subdir/x
  $ hg commit -qAm 'add subdir/x'
  $ cd ..

Create client2 - it will have only the first commit, so client1 will be pushing
two server and one local commits later.
  $ hgcloneshallow ssh://user@dummy/master client2 -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ cat >> client2/.hg/hgrc <<EOF
  > [extensions]
  > fastmanifest=
  > treemanifest=
  > 
  > [remotefilelog]
  > reponame=master
  > cachepath=$CACHEDIR
  > usefastdatapack=True
  > 
  > [fastmanifest]
  > usetree=True
  > usecache=False
  > EOF

Create create two more server commits
  $ cd master
  $ echo x >> subdir/x
  $ hg commit -m 'modify subdir/x'
  $ echo x >> subdir/x
  $ hg commit -m 'modify subdir/x again'
  $ cd ..

Create client1 - it will have both server commits
  $ hgcloneshallow ssh://user@dummy/master client1 -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ cd client1
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > fastmanifest=
  > treemanifest=
  > 
  > [remotefilelog]
  > reponame=master
  > cachepath=$CACHEDIR
  > usefastdatapack=True
  > 
  > [fastmanifest]
  > usetree=True
  > usecache=False
  > EOF

  $ echo a > a
  $ mkdir dir
  $ echo b > dir/b
  $ hg commit -Aqm 'initial commit'
  2 trees fetched over * (glob)

  $ ls .hg/store/packs/manifests
  a235931ce2211c58acacdf765f4050d5c92a54e5.dataidx
  a235931ce2211c58acacdf765f4050d5c92a54e5.datapack
  fedab9b2d171527f5d1109c27e8ba5dc80b36f6c.histidx
  fedab9b2d171527f5d1109c27e8ba5dc80b36f6c.histpack

Pushing with treemanifest disabled does not produce trees
(disable demand import so treemanifest.py is forced to load)
  $ HGDEMANDIMPORT=disable hg push -q ../client2 --config extensions.treemanifest=! --config fastmanifest.usetree=False
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ ls ../client2/.hg/store/packs/manifests || true
  * $ENOENT$ (glob)

  $ hg -R ../client2 strip -q -r 'tip^^' --config extensions.treemanifest=! --config fastmanifest.usetree=False
  $ rm -rf ../client2/.hg/store/packs
  $ clearcache

Pushing with sendtrees=False does nothing and doesnt download any trees to the
cache.
# Prefetch client2 so we dont see any downloads related to what the target
# already has.
  $ hg -R ../client2 prefetch -r 'all()'
  2 trees fetched over * (glob)
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
# Push and expect only one bulk download of trees
  $ hg push -q ../client2
  4 trees fetched over * (glob)
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over * (glob)
  $ ls_l $CACHEDIR/master/packs/manifests
  -r--r--r--    1186 06f87527833ba45e5e277a5acf0a20d9e6ec2671.dataidx
  -r--r--r--     421 06f87527833ba45e5e277a5acf0a20d9e6ec2671.datapack
  -r--r--r--    1196 1b69dc04d7f9d9825351f0af940c80f956e372b9.histidx
  -r--r--r--     183 1b69dc04d7f9d9825351f0af940c80f956e372b9.histpack
  -r--r--r--    1106 4d21ecb6c95e12dcf807b793cd1c55eeed861734.dataidx
  -r--r--r--     211 4d21ecb6c95e12dcf807b793cd1c55eeed861734.datapack
  -r--r--r--    1252 940bb8bf7ddf4196fff7fd1e837cbed98cb19c19.histidx
  -r--r--r--     347 940bb8bf7ddf4196fff7fd1e837cbed98cb19c19.histpack
  $ hg -R ../client2 strip -q -r 'tip^^' --config extensions.treemanifest=! --config fastmanifest.usetree=False
  $ rm -rf ../client2/.hg/store/packs
  $ clearcache

Pushing p2p with sendtrees=True puts the received packs in the local pack store
# Prefetch client2 so we dont see any downloads related to what the target
# already has.
  $ hg -R ../client2 prefetch -r 'all()'
  2 trees fetched over * (glob)
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
# Push and expect only one bulk download of trees
  $ hg push -q ../client2 --config treemanifest.sendtrees=True
  4 trees fetched over * (glob)
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over * (glob)
  $ ls ../client2/.hg/store/packs/manifests
  52e40524fd1590fae9864853645b640b33e9cab4.dataidx
  52e40524fd1590fae9864853645b640b33e9cab4.datapack
  b838127394a8b2e240f6041002d2d8ba20666e3e.histidx
  b838127394a8b2e240f6041002d2d8ba20666e3e.histpack
  $ hg debughistorypack ../client2/.hg/store/packs/manifests/*histidx
  
  
  Node          P1 Node       P2 Node       Link Node     Copy From
  3ffa0e0bbc70  90044db98b33  000000000000  000000000000  
  90044db98b33  d9920715ba88  000000000000  b8ff91c925b4  
  d9920715ba88  85b359fdb09e  000000000000  2937cde31c19  
  
  dir
  Node          P1 Node       P2 Node       Link Node     Copy From
  23226e7a252c  000000000000  000000000000  000000000000  
  
  subdir
  Node          P1 Node       P2 Node       Link Node     Copy From
  143a95c22d77  a18d21674e76  000000000000  b8ff91c925b4  
  a18d21674e76  bc0c2c938b92  000000000000  2937cde31c19  
