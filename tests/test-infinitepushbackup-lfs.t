
Setup common infinitepush
  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/infinitepush/library.sh"
  $ setupcommon

Setup lfs
  $ cat >> $HGRCPATH << EOF
  > [experimental]
  > changegroup3=True
  > [extensions]
  > lfs=
  > [lfs]
  > threshold=10B
  > url=file:$TESTTMP/dummy-remote/
  > EOF

Setup server repo
  $ hg init repo
  $ cd repo
  $ setupserver
  $ echo 1 > 1
  $ hg add 1
  $ hg ci -m initial

Setup client
  $ cd ..
  $ hg clone ssh://user@dummy/repo client -q
  $ cd client
  $ echo aaaaaaaaaaa > largefile
  $ hg ci -Aqm commit
  $ hg debugdata largefile 0
  version https://git-lfs.github.com/spec/v1
  oid sha256:ab483e1d855ad0ea27a68eeea02a04c1de6ccd2dc2c05e3a48c9a1ebb8af5f99
  size 12
  x-is-binary 0

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
  $ hg update scratch/lfscommit
  'scratch/lfscommit' does not exist locally - looking for it remotely...
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets 0da81a72db1a
  (run 'hg update' to get a working copy)
  'scratch/lfscommit' found remotely
  pull finished in * sec (glob)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark scratch/lfscommit)

Make pushbackup that contains bundle with 2 heads
  $ cd ../client
  $ hg up -q tip
  $ mkcommit newcommit
  $ hg up -q 0
  $ mkcommit newcommit2
  $ hg pushbackup
  starting backup * (glob)
  backing up stack rooted at 0da81a72db1a
  remote: pushing 2 commits:
  remote:     0da81a72db1a  commit
  remote:     5f9d85f9e1c6  newcommit
  backing up stack rooted at eca66fbd9785
  remote: pushing 1 commit:
  remote:     eca66fbd9785  newcommit2
  finished in * seconds (glob)
  $ hg isbackedup -r .
  eca66fbd9785d8e82fb4043a2ed9beed8bdbce5b backed up

Pull just one head to trigger rebundle
  $ cd ../client2
  $ hg pull -r eca66fbd9785d8e82fb4043a2ed9beed8bdbce5b
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  new changesets eca66fbd9785
  (run 'hg heads' to see heads, 'hg merge' to merge)
