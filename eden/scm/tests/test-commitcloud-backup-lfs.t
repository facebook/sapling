#debugruntest-compatible

#require no-eden

#inprocess-hg-incompatible
  $ setconfig experimental.allowfilepeer=True

  $ enable amend remotenames

Setup common infinitepush
  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/infinitepush/library.sh"
  $ setupcommon

Setup lfs
  $ setconfig remotefilelog.lfs=true
  $ setconfig experimental.changegroup3=true
  $ setconfig lfs.threshold=10B lfs.url="file:$TESTTMP/dummy-remote/"

Setup server repo
  $ hg init repo
  $ cd repo
  $ setupserver
  $ echo 1 > 1
  $ hg add 1
  $ hg ci -m initial
  $ hg bookmark master

Setup client
  $ cd ..
  $ hg clone ssh://user@dummy/repo client -q
  $ cd client
  $ echo aaaaaaaaaaa > largefile
  $ hg ci -Aqm commit

  $ hg push -r . --to lfscommit --create
  pushing rev 0da81a72db1a to destination ssh://user@dummy/repo bookmark lfscommit
  searching for changes
  exporting bookmark lfscommit
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes

  $ cd ..

Setup another client
  $ hg clone ssh://user@dummy/repo client2 -q
  $ cd client2
  $ hg goto lfscommit
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

Make pushbackup that contains bundle with 2 heads
  $ cd ../client
  $ hg up -q tip
  $ mkcommit newcommit
  $ hg prev -q
  [0da81a] commit
  $ mkcommit newcommit2
  $ hg cloud backup
  commitcloud: head '5f9d85f9e1c6' hasn't been uploaded yet
  commitcloud: head 'c800524c1b76' hasn't been uploaded yet
  edenapi: queue 2 commits for upload
  edenapi: queue 2 files for upload
  edenapi: uploaded 2 files
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 2 changesets
  $ hg cloud check -r .
  c800524c1b7637c6f3f997d1459237d01fe1ea10 backed up

Pull just one head to trigger rebundle
  $ cd ../client2
  $ hg pull -r c800524c1b7637c6f3f997d1459237d01fe1ea10
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
