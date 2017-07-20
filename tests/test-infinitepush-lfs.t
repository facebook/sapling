
Setup common infinitepush
  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/library-infinitepush.sh"
  $ setupcommon

Setup lfs
  $ cat >> $HGRCPATH << EOF
  > [experimental]
  > changegroup3=True
  > [extensions]
  > lfs=$TESTDIR/../hgext3rd/lfs/
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
  (run 'hg update' to get a working copy)
  'scratch/lfscommit' found remotely
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark scratch/lfscommit)
