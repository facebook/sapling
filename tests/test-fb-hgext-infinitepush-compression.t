  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/infinitepush/library.sh"
  $ setupcommon
  $ cat << EOF >> $HGRCPATH
  > [infinitepush]
  > bundlecompression = GZ
  > EOF

Setup server
  $ hg init repo
  $ cd repo
  $ setupserver
  $ cd ..

Backup a commit
  $ hg clone ssh://user@dummy/repo client -q
  $ cd client
  $ mkcommit commit
  $ hg pushbackup
  starting backup .* (re)
  searching for changes
  remote: pushing 1 commit:
  remote:     7e6a6fd9c7c8  commit
  finished in \d+\.(\d+)? seconds (re)

Check the commit is compressed
  $ f=`cat ../repo/.hg/scratchbranches/index/nodemap/7e6a6fd9c7c8c8c307ee14678f03d63af3a7b455`
  $ hg debugbundle ../repo/.hg/scratchbranches/filebundlestore/*/*/$f
  Stream params: {Compression: GZ}
  changegroup -- {version: 02}
      7e6a6fd9c7c8c8c307ee14678f03d63af3a7b455
