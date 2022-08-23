#chg-compatible
#debugruntest-compatible
  $ configure modernclient

  $ newserver master
  $ setconfig extensions.lfs= lfs.url=file:$TESTTMP/lfs-server remotefilelog.lfs=True

  $ clone master shallow --noupdate
  $ switchrepo shallow
  $ setconfig extensions.lfs= lfs.url=file:$TESTTMP/lfs-server lfs.threshold=10B remotefilelog.lfs=True

  $ echo "THIS IS AN LFS BLOB" > x
  $ hg commit -qAm x

# Make sure that bundle isn't confused by this.
  $ hg bundle -q -r . $TESTTMP/test-bundle

  $ clone master shallow2 --noupdate
  $ switchrepo shallow2
  $ setconfig remotefilelog.lfs=True lfs.url=file:$TESTTMP/lfs-server lfs.threshold=10GB

  $ hg unbundle -q -u $TESTTMP/test-bundle
  $ cat x
  THIS IS AN LFS BLOB
