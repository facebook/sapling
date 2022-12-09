#chg-compatible
  $ setconfig format.use-segmented-changelog=false
  $ setconfig devel.segmented-changelog-rev-compat=true
  $ setconfig workingcopy.ruststatus=False
  $ setconfig experimental.allowfilepeer=True

  $ configure modern
  $ setconfig remotefilelog.write-hgcache-to-indexedlog=False remotefilelog.write-local-to-indexedlog=False
# test relies on pack files
  $ setconfig scmstore.enableshim=False

  $ newserver master
  $ setconfig extensions.lfs= lfs.url=file:$TESTTMP/lfs-server
  $ cd $TESTTMP

  $ clone master shallow --noupdate
  $ cd shallow
  $ setconfig remotefilelog.useruststore=True remotefilelog.localdatarepack=True lfs.moveafterupload=True

# First, let's generate some LFS blobs on the server
  $ setconfig extensions.lfs= lfs.threshold=10B lfs.url=file:$TESTTMP/lfs-server/

  $ echo "THIS IS AN LFS BLOB" > x
  $ hg commit -qAm x

# Make sure that when remotefilelog.lfs is enabled, we can still read the blob properly
  $ setconfig remotefilelog.lfs=True

# Verify that without the one-time repack, we can't read the LFS blobs.
  $ hg log -p -r . 2> /dev/null
  [1]

# Now do the one time repack
  $ setconfig remotefilelog.maintenance.timestamp.localrepack=1 remotefilelog.maintenance=localrepack
  $ hg log -p -r .
  Running a one-time local repack, this may take some time
  Done with one-time local repack
  commit:      ab1b0b8595ed
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     x
  
  diff -r 000000000000 -r ab1b0b8595ed x
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/x	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +THIS IS AN LFS BLOB
  

  $ hg up null
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg up -r tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat x
  THIS IS AN LFS BLOB

  $ echo "THIS IS ANOTHER LFS BLOB" > y
  $ hg commit -qAm y

# No datapack remaining due to the full repack done above
  $ find .hg/store/packs -type f | sort
  .hg/store/packs/2fcfe5e792f7f55c4f39486d348654be30a5934a.histidx
  .hg/store/packs/2fcfe5e792f7f55c4f39486d348654be30a5934a.histpack
  .hg/store/packs/dcb97073fda83c4a025153d7b929406f4d86e188.histidx
  .hg/store/packs/dcb97073fda83c4a025153d7b929406f4d86e188.histpack
  .hg/store/packs/manifests/9bb7cc2e0e433f3564cbef21705ff896d9be2473.histidx
  .hg/store/packs/manifests/9bb7cc2e0e433f3564cbef21705ff896d9be2473.histpack
  .hg/store/packs/manifests/b00bb3b75ccfe82ff2ac879b3b323c8005835d1a.dataidx
  .hg/store/packs/manifests/b00bb3b75ccfe82ff2ac879b3b323c8005835d1a.datapack
  .hg/store/packs/manifests/da1b23b92928a4cf48dc85136fde02a3b90cc657.histidx
  .hg/store/packs/manifests/da1b23b92928a4cf48dc85136fde02a3b90cc657.histpack

  $ find .hg/store/lfs/objects -type f | sort
  .hg/store/lfs/objects/e4/1d3fc42af9a3407f07926c75946c0aa433ccbd99c175b98474fa19b2ee5963
  .hg/store/lfs/objects/f3/8ef89300956a8cf001746d6e4b015708c3d0d883d1a69bf00f4958090cbe21

  $ hg push -q --to master --create
  $ find $TESTTMP/lfs-server -type f | sort
  $TESTTMP/lfs-server/e4/1d3fc42af9a3407f07926c75946c0aa433ccbd99c175b98474fa19b2ee5963
  $TESTTMP/lfs-server/f3/8ef89300956a8cf001746d6e4b015708c3d0d883d1a69bf00f4958090cbe21

# Both blobs were in the LFS store, and thus have been uploaded and moved to the shared store.
  $ find .hg/store/lfs/objects -type f | sort

# No datapack should be present.
  $ find .hg/store/packs -type f | sort
  .hg/store/packs/2fcfe5e792f7f55c4f39486d348654be30a5934a.histidx
  .hg/store/packs/2fcfe5e792f7f55c4f39486d348654be30a5934a.histpack
  .hg/store/packs/dcb97073fda83c4a025153d7b929406f4d86e188.histidx
  .hg/store/packs/dcb97073fda83c4a025153d7b929406f4d86e188.histpack
  .hg/store/packs/manifests/9bb7cc2e0e433f3564cbef21705ff896d9be2473.histidx
  .hg/store/packs/manifests/9bb7cc2e0e433f3564cbef21705ff896d9be2473.histpack
  .hg/store/packs/manifests/b00bb3b75ccfe82ff2ac879b3b323c8005835d1a.dataidx
  .hg/store/packs/manifests/b00bb3b75ccfe82ff2ac879b3b323c8005835d1a.datapack
  .hg/store/packs/manifests/da1b23b92928a4cf48dc85136fde02a3b90cc657.histidx
  .hg/store/packs/manifests/da1b23b92928a4cf48dc85136fde02a3b90cc657.histpack

# And verify we can read the blobs properly
  $ hg up null
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg up -r tip
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat x
  THIS IS AN LFS BLOB
  $ cat y
  THIS IS ANOTHER LFS BLOB

  $ cd ..
  $ rm -rf shallow

  $ clone master shallow --noupdate
  $ cd shallow
  $ setconfig remotefilelog.useruststore=True remotefilelog.localdatarepack=True lfs.moveafterupload=True

# Let's validate that both the LFS extension, and the remotefilelog LFS can co-exist when pulling blobs
  $ setconfig extensions.lfs= lfs.threshold=10B lfs.url=file:$TESTTMP/lfs-server/ remotefilelog.lfs=True

  $ hg goto -r tip
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat x
  THIS IS AN LFS BLOB
  $ cat y
  THIS IS ANOTHER LFS BLOB
  $ find $TESTTMP/default-hgcache -type f | sort
  $TESTTMP/default-hgcache/master/indexedlogdatastore/0/index2-node
  $TESTTMP/default-hgcache/master/indexedlogdatastore/0/log
  $TESTTMP/default-hgcache/master/indexedlogdatastore/0/meta
  $TESTTMP/default-hgcache/master/indexedlogdatastore/0/rlock
  $TESTTMP/default-hgcache/master/indexedlogdatastore/latest
  $TESTTMP/default-hgcache/master/indexedlogdatastore/rlock
  $TESTTMP/default-hgcache/master/indexedloghistorystore/0/index2-node_and_path
  $TESTTMP/default-hgcache/master/indexedloghistorystore/0/log
  $TESTTMP/default-hgcache/master/indexedloghistorystore/0/meta
  $TESTTMP/default-hgcache/master/indexedloghistorystore/0/rlock
  $TESTTMP/default-hgcache/master/indexedloghistorystore/latest
  $TESTTMP/default-hgcache/master/indexedloghistorystore/rlock
  $TESTTMP/default-hgcache/master/lfs/blobs/0/index2-sha256
  $TESTTMP/default-hgcache/master/lfs/blobs/0/log
  $TESTTMP/default-hgcache/master/lfs/blobs/0/meta
  $TESTTMP/default-hgcache/master/lfs/blobs/0/rlock
  $TESTTMP/default-hgcache/master/lfs/blobs/latest
  $TESTTMP/default-hgcache/master/lfs/blobs/rlock
  $TESTTMP/default-hgcache/master/lfs/pointers/0/index2-node
  $TESTTMP/default-hgcache/master/lfs/pointers/0/index2-sha256
  $TESTTMP/default-hgcache/master/lfs/pointers/0/log
  $TESTTMP/default-hgcache/master/lfs/pointers/0/meta
  $TESTTMP/default-hgcache/master/lfs/pointers/0/rlock
  $TESTTMP/default-hgcache/master/lfs/pointers/latest
  $TESTTMP/default-hgcache/master/lfs/pointers/rlock
  $TESTTMP/default-hgcache/master/manifests/indexedlogdatastore/0/index2-node
  $TESTTMP/default-hgcache/master/manifests/indexedlogdatastore/0/log
  $TESTTMP/default-hgcache/master/manifests/indexedlogdatastore/0/meta
  $TESTTMP/default-hgcache/master/manifests/indexedlogdatastore/0/rlock
  $TESTTMP/default-hgcache/master/manifests/indexedlogdatastore/latest
  $TESTTMP/default-hgcache/master/manifests/indexedlogdatastore/rlock
  $TESTTMP/default-hgcache/master/manifests/indexedloghistorystore/0/index2-node_and_path
  $TESTTMP/default-hgcache/master/manifests/indexedloghistorystore/0/log
  $TESTTMP/default-hgcache/master/manifests/indexedloghistorystore/0/meta
  $TESTTMP/default-hgcache/master/manifests/indexedloghistorystore/0/rlock
  $TESTTMP/default-hgcache/master/manifests/indexedloghistorystore/latest
  $TESTTMP/default-hgcache/master/manifests/indexedloghistorystore/rlock
  $TESTTMP/default-hgcache/master/manifests/lfs/blobs/0/index2-sha256
  $TESTTMP/default-hgcache/master/manifests/lfs/blobs/0/log
  $TESTTMP/default-hgcache/master/manifests/lfs/blobs/0/meta
  $TESTTMP/default-hgcache/master/manifests/lfs/blobs/0/rlock
  $TESTTMP/default-hgcache/master/manifests/lfs/blobs/latest
  $TESTTMP/default-hgcache/master/manifests/lfs/blobs/rlock
  $TESTTMP/default-hgcache/master/manifests/lfs/pointers/0/index2-node
  $TESTTMP/default-hgcache/master/manifests/lfs/pointers/0/index2-sha256
  $TESTTMP/default-hgcache/master/manifests/lfs/pointers/0/log
  $TESTTMP/default-hgcache/master/manifests/lfs/pointers/0/meta
  $TESTTMP/default-hgcache/master/manifests/lfs/pointers/0/rlock
  $TESTTMP/default-hgcache/master/manifests/lfs/pointers/latest
  $TESTTMP/default-hgcache/master/manifests/lfs/pointers/rlock
  $TESTTMP/default-hgcache/master/packs/cdd6b0d104ad718cec29643359c67c56c91e483e.histidx
  $TESTTMP/default-hgcache/master/packs/cdd6b0d104ad718cec29643359c67c56c91e483e.histpack
  $TESTTMP/default-hgcache/master/packs/manifests/9bb7cc2e0e433f3564cbef21705ff896d9be2473.histidx
  $TESTTMP/default-hgcache/master/packs/manifests/9bb7cc2e0e433f3564cbef21705ff896d9be2473.histpack

# Disable the remotefilelog LFS implementation to verify we can still read the LFS blobs properly.
  $ setconfig remotefilelog.lfs=False
  $ hg up null
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg up -r tip
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat x
  THIS IS AN LFS BLOB
  $ cat y
  THIS IS ANOTHER LFS BLOB
