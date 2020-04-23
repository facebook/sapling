#chg-compatible

  $ . "$TESTDIR/library.sh"

  $ newserver master
  $ setconfig extensions.lfs= lfs.url=file:$TESTTMP/lfs-server
  $ cd $TESTTMP

  $ clone master shallow --noupdate
  $ cd shallow
  $ setconfig remotefilelog.useruststore=True worker.rustworkers=True remotefilelog.localdatarepack=True

# First, let's generate some LFS blobs on the server
  $ setconfig extensions.lfs= lfs.threshold=10B lfs.url=file:$TESTTMP/lfs-server/

  $ echo "THIS IS AN LFS BLOB" > x
  $ hg commit -qAm x

# Make sure that when remotefilelog.lfs is enabled, we can still read the blob properly
  $ setconfig remotefilelog.lfs=True
  $ hg up null
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg up -r tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat x
  THIS IS AN LFS BLOB

  $ echo "THIS IS ANOTHER LFS BLOB" > y
  $ hg commit -qAm y

# Only the first LFS blobs is created via the LFS extension, ie: one datapack.
  $ findfilessorted .hg/store/packs
  .hg/store/packs/2fcfe5e792f7f55c4f39486d348654be30a5934a.histidx
  .hg/store/packs/2fcfe5e792f7f55c4f39486d348654be30a5934a.histpack
  .hg/store/packs/537800aabca8d55a85249dc165e50c4fd2a447a7.dataidx
  .hg/store/packs/537800aabca8d55a85249dc165e50c4fd2a447a7.datapack
  .hg/store/packs/dcb97073fda83c4a025153d7b929406f4d86e188.histidx
  .hg/store/packs/dcb97073fda83c4a025153d7b929406f4d86e188.histpack
  .hg/store/packs/manifests/1ad2258b3968784028da4c7af67e58472ed95148.dataidx
  .hg/store/packs/manifests/1ad2258b3968784028da4c7af67e58472ed95148.datapack
  .hg/store/packs/manifests/9bb7cc2e0e433f3564cbef21705ff896d9be2473.histidx
  .hg/store/packs/manifests/9bb7cc2e0e433f3564cbef21705ff896d9be2473.histpack
  .hg/store/packs/manifests/b00bb3b75ccfe82ff2ac879b3b323c8005835d1a.dataidx
  .hg/store/packs/manifests/b00bb3b75ccfe82ff2ac879b3b323c8005835d1a.datapack
  .hg/store/packs/manifests/da1b23b92928a4cf48dc85136fde02a3b90cc657.histidx
  .hg/store/packs/manifests/da1b23b92928a4cf48dc85136fde02a3b90cc657.histpack

  $ hg push -q --to master --create
  $ findfilessorted $TESTTMP/lfs-server
  $TESTTMP/lfs-server/e4/1d3fc42af9a3407f07926c75946c0aa433ccbd99c175b98474fa19b2ee5963
  $TESTTMP/lfs-server/f3/8ef89300956a8cf001746d6e4b015708c3d0d883d1a69bf00f4958090cbe21

# Now let's repack to move the LFS pointer to the remotefilelog LFS pointer store.
  $ hg repack

# No datapack should be present.
  $ findfilessorted .hg/store/packs
  .hg/store/packs/cdd6b0d104ad718cec29643359c67c56c91e483e.histidx
  .hg/store/packs/cdd6b0d104ad718cec29643359c67c56c91e483e.histpack
  .hg/store/packs/manifests/2eece36afda8d776ef688304461f9bf904379e3e.dataidx
  .hg/store/packs/manifests/2eece36afda8d776ef688304461f9bf904379e3e.datapack
  .hg/store/packs/manifests/6f96b59e7b3c62bf2d3aba403f72343b53c05d95.histidx
  .hg/store/packs/manifests/6f96b59e7b3c62bf2d3aba403f72343b53c05d95.histpack

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
  $ setconfig remotefilelog.useruststore=True worker.rustworkers=True remotefilelog.localdatarepack=True

# Let's validate that both the LFS extension, and the remotefilelog LFS can co-exist when pulling blobs
  $ setconfig extensions.lfs= lfs.threshold=10B lfs.url=file:$TESTTMP/lfs-server/ remotefilelog.lfs=True

  $ hg update -r tip
  fetching tree '' 11643b8969ec3aa286b2209783159cee526e902a, found via 316bb9353f6a
  1 trees fetched over 0.00s
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat x
  THIS IS AN LFS BLOB
  $ cat y
  THIS IS ANOTHER LFS BLOB
  $ findfilessorted $CACHEDIR
  $TESTTMP/hgcache/master/lfs/blobs/0/index2-sha256
  $TESTTMP/hgcache/master/lfs/blobs/0/log
  $TESTTMP/hgcache/master/lfs/blobs/0/meta
  $TESTTMP/hgcache/master/lfs/blobs/latest
  $TESTTMP/hgcache/master/lfs/pointers/0/index2-node
  $TESTTMP/hgcache/master/lfs/pointers/0/index2-sha256
  $TESTTMP/hgcache/master/lfs/pointers/0/log
  $TESTTMP/hgcache/master/lfs/pointers/0/meta
  $TESTTMP/hgcache/master/lfs/pointers/latest
  $TESTTMP/hgcache/master/packs/cdd6b0d104ad718cec29643359c67c56c91e483e.histidx
  $TESTTMP/hgcache/master/packs/cdd6b0d104ad718cec29643359c67c56c91e483e.histpack
  $TESTTMP/hgcache/master/packs/manifests/1ad2258b3968784028da4c7af67e58472ed95148.dataidx
  $TESTTMP/hgcache/master/packs/manifests/1ad2258b3968784028da4c7af67e58472ed95148.datapack
  $TESTTMP/hgcache/master/packs/manifests/9bb7cc2e0e433f3564cbef21705ff896d9be2473.histidx
  $TESTTMP/hgcache/master/packs/manifests/9bb7cc2e0e433f3564cbef21705ff896d9be2473.histpack
  $TESTTMP/hgcache/master/packs/repacklock

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
