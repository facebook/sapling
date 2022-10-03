#chg-compatible
  $ setconfig workingcopy.ruststatus=False
  $ setconfig experimental.allowfilepeer=True

  $ . "$TESTDIR/library.sh"

  $ newserver master
  $ cd $TESTTMP
  $ clone master shallow --noupdate
  $ cd shallow

  $ setconfig remotefilelog.useruststore=True remotefilelog.lfs=True
  $ setconfig lfs.url=file:$TESTTMP/lfs-server

  $ echo "X" > x
  $ hg commit -qAm x
  $ echo "Y" > y
  $ hg commit -qAm y

  $ hg bundle -r tip --base tip~1 out.bundle
  1 changesets found
