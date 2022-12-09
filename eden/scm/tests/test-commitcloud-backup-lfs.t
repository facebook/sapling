#chg-compatible
#debugruntest-compatible
#inprocess-hg-incompatible
  $ setconfig experimental.allowfilepeer=True

  $ enable amend

Setup common infinitepush
  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/infinitepush/library.sh"
  $ setupcommon

Setup lfs
  $ enable lfs
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

  $ hg push -r . --to scratch/lfscommit --create
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: pushing 1 commit:
  remote:     0da81a72db1a  commit

  $ scratchbookmarks
  scratch/lfscommit 0da81a72db1a2d8256845e3808971f33e73d24c4

  $ cd ..

Setup another client
  $ hg clone ssh://user@dummy/repo client2 -q
  $ cd client2
  $ hg goto scratch/lfscommit
  pulling 'scratch/lfscommit' from 'ssh://user@dummy/repo'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Make pushbackup that contains bundle with 2 heads
  $ cd ../client
  $ hg up -q tip
  $ mkcommit newcommit
  $ hg prev -q
  [0da81a] commit
  $ mkcommit newcommit2
  $ hg cloud backup
  backing up stack rooted at 0da81a72db1a
  commitcloud: backed up 2 commits
  remote: pushing 3 commits:
  remote:     0da81a72db1a  commit
  remote:     5f9d85f9e1c6  newcommit
  remote:     c800524c1b76  newcommit2
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
