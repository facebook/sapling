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
  fetching tree '' 90044db98b33ed191d9e056e2c2ec65ae7af8338, found via b8ff91c925b4
  2 trees fetched over * (glob)

  $ ls .hg/store/packs/manifests
  53e6d2d846d94f543bad25dcbaa1f753c3ce9fa6.histidx
  53e6d2d846d94f543bad25dcbaa1f753c3ce9fa6.histpack
  a235931ce2211c58acacdf765f4050d5c92a54e5.dataidx
  a235931ce2211c58acacdf765f4050d5c92a54e5.datapack

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
# Push and expect downloads of both public trees (in arbitrary order)
  $ hg push -q ../client2 --config treemanifest.sendtrees=True --config treemanifest.treeonly=True
  fetching tree '' *, based on 85b359fdb09e9b8d7ac4a74551612b277345e8fd, found via 54609f68e211 (glob)
  2 trees fetched over * (glob)
  fetching tree '' *, based on *, found via 54609f68e211 (glob)
  2 trees fetched over * (glob)
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over * (glob)
  $ ls ../client2/.hg/store/packs/manifests
  700fb8a7918068e308a998d338d1689074118d07.histidx
  700fb8a7918068e308a998d338d1689074118d07.histpack
  c446da942a8eb5a687e11a12920e4d4526ef765a.dataidx
  c446da942a8eb5a687e11a12920e4d4526ef765a.datapack
  $ hg debughistorypack ../client2/.hg/store/packs/manifests/*histidx
  
  
  Node          P1 Node       P2 Node       Link Node     Copy From
  3ffa0e0bbc70  90044db98b33  000000000000  54609f68e211  
  
  dir
  Node          P1 Node       P2 Node       Link Node     Copy From
  23226e7a252c  000000000000  000000000000  54609f68e211  
  
  subdir
  Node          P1 Node       P2 Node       Link Node     Copy From
  143a95c22d77  a18d21674e76  000000000000  b8ff91c925b4  

Pulling between peers should send local trees but not remote trees
# Strip back one server commit and one draft commit, so we can pull them again
  $ cd ../client2
  $ hg strip -r 2 --no-backup
# Delete the old local tree data from the draft commit, so we can verify it is
# downloaded again during pull.
  $ rm -rf .hg/store/packs/*
# Change this client to use a different cache from the other client, since the
# other client may populate data that we need to test if this client is
# downloading.
  $ cp .hg/hgrc .hg/hgrc.bak
  $ mkdir $TESTTMP/hgcache2
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > cachepath=$TESTTMP/hgcache2
  > EOF
# Force the draft commit to be public so we can ensure it's trees are delivered
# despite it being public.
  $ hg -R ../client1 phase -r 3 -p
  $ rm -rf $TESTTMP/hgcache2/*
  $ hg pull -q --config treemanifest.sendtrees=True ../client1 --config remotefilelog.fallbackpath=ssh://user@dummy/master
# Check that the local commits for the previously-draft-but-now-public commit
# were downloaded to the local store.
  $ ls_l .hg/store/packs/manifests
  -r--r--r--    1273 700fb8a7918068e308a998d338d1689074118d07.histidx
  -r--r--r--     274 700fb8a7918068e308a998d338d1689074118d07.histpack
  -r--r--r--    1146 c446da942a8eb5a687e11a12920e4d4526ef765a.dataidx
  -r--r--r--     402 c446da942a8eb5a687e11a12920e4d4526ef765a.datapack
  $ hg debugdatapack .hg/store/packs/manifests/*.datapack
  .hg/store/packs/manifests/c446da942a8eb5a687e11a12920e4d4526ef765a:
  dir:
  Node          Delta Base    Delta Length  Blob Size
  23226e7a252c  000000000000  43            (missing)
  
  subdir:
  Node          Delta Base    Delta Length  Blob Size
  143a95c22d77  000000000000  43            (missing)
  
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  3ffa0e0bbc70  000000000000  138           (missing)
  
# Verify the real-public commit wasn't received during the pull and therefore
# has to be downloaded on demand.
  $ rm -rf $TESTTMP/hgcache2/*
  $ ls_l $TESTTMP/hgcache2/
  $ hg manifest -r 'tip^'
  fetching tree '' 90044db98b33ed191d9e056e2c2ec65ae7af8338, based on 3ffa0e0bbc707f62567ec13cb2dd919bef47aa97, found via b8ff91c925b4
  2 trees fetched over * (glob)
  subdir/x
# Verify the fake-public commit was received during the pull and does not
# require additional ondemand downloads.
  $ hg manifest -r tip
  a
  dir/b
  subdir/x
  $ mv .hg/hgrc.bak .hg/hgrc
