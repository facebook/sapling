#chg-compatible
  $ setconfig format.use-segmented-changelog=false
  $ setconfig devel.segmented-changelog-rev-compat=true
  $ setconfig experimental.allowfilepeer=True

  $ configure modern

  $ setconfig extensions.lfs= lfs.threshold=10B lfs.url=file:$TESTTMP/lfs-server

  $ newserver server
  $ cd $TESTTMP

  $ newremoterepo
  $ setconfig lfs.moveafterupload=True

# First, let's generate some LFS blobs on the server

  $ echo "THIS IS AN LFS BLOB" > x
  $ hg commit -qAm x

# Make sure that when remotefilelog.lfs is enabled, we can still read the blob properly
  $ setconfig remotefilelog.lfs=True
  $ hg log -p -r .
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

  $ find .hg/store/lfs/objects -type f | sort
  .hg/store/lfs/objects/e4/1d3fc42af9a3407f07926c75946c0aa433ccbd99c175b98474fa19b2ee5963
  .hg/store/lfs/objects/f3/8ef89300956a8cf001746d6e4b015708c3d0d883d1a69bf00f4958090cbe21

  $ hg push -q --to master --create
  $ find $TESTTMP/lfs-server -type f | sort
  $TESTTMP/lfs-server/e4/1d3fc42af9a3407f07926c75946c0aa433ccbd99c175b98474fa19b2ee5963
  $TESTTMP/lfs-server/f3/8ef89300956a8cf001746d6e4b015708c3d0d883d1a69bf00f4958090cbe21

# Both blobs were in the LFS store, and thus have been uploaded and moved to the shared store.
  $ find .hg/store/lfs/objects -type f | sort

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

  $ newremoterepo
  $ setconfig lfs.moveafterupload=True

# Let's validate that both the LFS extension, and the remotefilelog LFS can co-exist when pulling blobs
  $ setconfig remotefilelog.lfs=True

  $ hg pull -q
  $ hg goto master
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat x
  THIS IS AN LFS BLOB
  $ cat y
  THIS IS ANOTHER LFS BLOB
