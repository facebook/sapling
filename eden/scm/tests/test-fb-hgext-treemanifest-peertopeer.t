#chg-compatible

  $ CACHEDIR=`pwd`/hgcache

  $ . "$TESTDIR/library.sh"

Create server
  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
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
  fetching tree '' 85b359fdb09e9b8d7ac4a74551612b277345e8fd
  2 trees fetched over * (glob)
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob) (?)
  $ cat >> client2/.hg/hgrc <<EOF
  > [remotefilelog]
  > reponame=master
  > cachepath=$CACHEDIR
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
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob) (?)
  fetching tree '' 90044db98b33ed191d9e056e2c2ec65ae7af8338, found via b8ff91c925b4
  2 trees fetched over * (glob)
  $ cd client1
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > reponame=master
  > cachepath=$CACHEDIR
  > EOF

  $ echo a > a
  $ mkdir dir
  $ echo b > dir/b
  $ hg commit -Aqm 'initial commit'

  $ ls .hg/store/packs/manifests
  53e6d2d846d94f543bad25dcbaa1f753c3ce9fa6.histidx
  53e6d2d846d94f543bad25dcbaa1f753c3ce9fa6.histpack
  b7cac023ec87107fd7c501085ba31c96485d802d.dataidx
  b7cac023ec87107fd7c501085ba31c96485d802d.datapack

  $ clearcache

Pushing p2p with sendtrees=True puts the received packs in the local pack store
# Prefetch client2 so we dont see any downloads related to what the target
# already has.
  $ hg -R ../client2 prefetch -r 'all()'
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob) (?)
  fetching tree '' 85b359fdb09e9b8d7ac4a74551612b277345e8fd
  2 trees fetched over * (glob)
  $ cp ../client2/.hg/hgrc ../client2/.hg/hgrc.bak
  $ cat >> ../client2/.hg/hgrc <<EOF
  > [remotefilelog]
  > cachepath=$CACHEDIR/cache2
  > EOF

# Push and expect downloads of both public trees (in arbitrary order)
  $ hg push -q ssh://user@dummy/client2
  remote: 1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob) (?)
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over * (glob) (?)
  fetching tree '' d9920715ba88cbc7962c4dac9f20004aafd94ac8, found via 2937cde31c19
  2 trees fetched over 0.00s
  fetching tree '' 90044db98b33ed191d9e056e2c2ec65ae7af8338, found via b8ff91c925b4
  2 trees fetched over 0.00s
  $ ls ../client2/.hg/store/packs/manifests
  51e464ee849d281ededc123094852f7217b677b3.dataidx
  51e464ee849d281ededc123094852f7217b677b3.datapack
  8eb3321b2b6f800e1a8cc163471046748725a9bf.histidx
  8eb3321b2b6f800e1a8cc163471046748725a9bf.histpack
  $ mv ../client2/.hg/hgrc.bak ../client2/.hg/hgrc
  $ hg debughistorypack ../client2/.hg/store/packs/manifests/*histidx
  
  
  Node          P1 Node       P2 Node       Link Node     Copy From
  3ffa0e0bbc70  90044db98b33  000000000000  54609f68e211  
  90044db98b33  d9920715ba88  000000000000  b8ff91c925b4  
  d9920715ba88  85b359fdb09e  000000000000  2937cde31c19  
  
  dir
  Node          P1 Node       P2 Node       Link Node     Copy From
  23226e7a252c  000000000000  000000000000  54609f68e211  
  
  subdir
  Node          P1 Node       P2 Node       Link Node     Copy From
  143a95c22d77  a18d21674e76  000000000000  b8ff91c925b4  
  a18d21674e76  bc0c2c938b92  000000000000  2937cde31c19  

Pulling between peers should send local trees but not remote trees
# Strip back one server commit and one draft commit, so we can pull them again
  $ cd ../client2
  $ hg debugstrip -r 2 --no-backup
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
  $ rm -rf $TESTTMP/hgcache2/*
  $ hg pull -q --config treemanifest.sendtrees=True ../client1 --config remotefilelog.fallbackpath=ssh://user@dummy/master
# Check that the local commits for the draft commit were downloaded to the local store.
  $ ls_l .hg/store/packs/manifests
  -r--r--r--    1194 584f14ba73a87b958e438135ea033ac7fdbb2cf0.dataidx
  -r--r--r--     523 584f14ba73a87b958e438135ea033ac7fdbb2cf0.datapack
  -r--r--r--    1301 cc1beef66dec330fd4cf9dcf56707c8f57031a78.histidx
  -r--r--r--     356 cc1beef66dec330fd4cf9dcf56707c8f57031a78.histpack
  $ hg debugdatapack .hg/store/packs/manifests/*.datapack
  .hg/store/packs/manifests/584f14ba73a87b958e438135ea033ac7fdbb2cf0:
  subdir:
  Node          Delta Base    Delta Length  Blob Size
  143a95c22d77  000000000000  43            (missing)
  
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  90044db98b33  000000000000  49            (missing)
  
  dir:
  Node          Delta Base    Delta Length  Blob Size
  23226e7a252c  000000000000  43            (missing)
  
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  3ffa0e0bbc70  000000000000  138           (missing)
  
# Verify the real-public commit wasn't received during the pull and therefore
# has to be downloaded on demand.
  $ rm -rf $TESTTMP/hgcache2/*
  $ ls_l $TESTTMP/hgcache2/
  $ hg manifest -r 'tip^'
  subdir/x
# Verify the fake-public commit was received during the pull and does not
# require additional ondemand downloads.
  $ hg manifest -r tip
  a
  dir/b
  subdir/x
  $ mv .hg/hgrc.bak .hg/hgrc
