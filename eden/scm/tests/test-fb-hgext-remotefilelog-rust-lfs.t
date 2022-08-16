#chg-compatible
#debugruntest-compatible
  $ setconfig experimental.allowfilepeer=True

  $ . "$TESTDIR/library.sh"
  $ setconfig remotefilelog.write-hgcache-to-indexedlog=False remotefilelog.write-local-to-indexedlog=False

  $ newserver master
  $ clone master shallow
  $ cd shallow
  $ setconfig remotefilelog.useruststore=True
  $ setconfig remotefilelog.lfs=True lfs.threshold=10B lfs.url=file:$TESTTMP/lfs

  $ printf 'THIS IS A BINARY LFS BLOB\0' > blob

  $ hg commit -qAm lfs1

  $ echo 'THIS IS ANOTHER LFS BLOB' > blob
  $ hg commit -qAm lfs2

  $ printf 'THIS IS A BINARY LFS BLOB\0' > blob
  $ hg commit -qAm lfs3

  $ findfilessorted .hg/store/lfs
  .hg/store/lfs/objects/8f/942761dd32573780723b14df5e401224674aa5ac58ef9f1df275f0c561433b
  .hg/store/lfs/objects/f3/8ef89300956a8cf001746d6e4b015708c3d0d883d1a69bf00f4958090cbe21
  .hg/store/lfs/pointers/index2-node
  .hg/store/lfs/pointers/index2-sha256
  .hg/store/lfs/pointers/lock (?)
  .hg/store/lfs/pointers/log
  .hg/store/lfs/pointers/meta
  .hg/store/lfs/pointers/rlock

# Blobs shouldn't have changed
  $ hg diff -r . -r .~2

# Remove the blobs
  $ rm -rf .hg/store/lfs/blobs

# With the backing blobs gone, diff should not complain about missing blobs
  $ hg diff -r . -r .~2
