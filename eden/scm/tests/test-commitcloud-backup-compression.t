#chg-compatible

  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/infinitepush/library.sh"
  $ setupcommon
  $ setconfig infinitepush.bundlecompression=GZ

Setup server
  $ hg init repo
  $ cd repo
  $ setupserver
  $ cd ..

Backup a commit
  $ hg clone ssh://user@dummy/repo client -q
  $ cd client
  $ mkcommit commit
  $ hg cloud backup
  backing up stack rooted at 7e6a6fd9c7c8
  remote: pushing 1 commit:
  remote:     7e6a6fd9c7c8  commit
  commitcloud: backed up 1 commit

Check the commit is compressed
  $ f=`cat ../repo/.hg/scratchbranches/index/nodemap/7e6a6fd9c7c8c8c307ee14678f03d63af3a7b455`
  $ hg debugbundle ../repo/.hg/scratchbranches/filebundlestore/*/*/$f
  Stream params: {Compression: GZ}
  changegroup -- {version: 02}
      7e6a6fd9c7c8c8c307ee14678f03d63af3a7b455
