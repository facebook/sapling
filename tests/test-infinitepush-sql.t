#if no-osx
  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    hg ci -d "0 0" -m "$1"
  > }
  $ . "$TESTDIR/library-infinitepush.sh"
  $ setupcommon

With no configuration it should abort
  $ hg init server
  $ cd server
  $ setupsqlserverhgrc
  $ hg st
  abort: please set infinitepush.sqlhost
  [255]
  $ setupdb
  $ cd ..
  $ hg clone -q ssh://user@dummy/server client1
  $ hg clone -q ssh://user@dummy/server client2
  $ cd client1
  $ setupsqlclienthgrc
  $ cd ../client2
  $ setupsqlclienthgrc
  $ cd ../client1
  $ mkcommit scratchcommit

  $ hg push -r . --to scratch/book --create
  pushing to ssh://user@dummy/server
  searching for changes
  remote: pushing 1 commit:
  remote:     2d9cfa751213  scratchcommit

Make pull and check that scratch commit is not pulled
  $ cd ../client2
  $ hg pull
  pulling from ssh://user@dummy/server
  no changes found
  $ hg up scratch/book
  abort: unknown revision 'scratch/book'!
  [255]

Pull scratch commit from the second client
  $ hg pull -B scratch/book
  pulling from ssh://user@dummy/server
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)
  $ hg up scratch/book
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark scratch/book)
  $ hg log -G
  @  changeset:   0:2d9cfa751213
     bookmark:    scratch/book
     tag:         tip
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     scratchcommit
  
  $ cd ../server
  $ hg book scratch/%erversidebook
  $ hg book serversidebook
  $ cd ../client1
  $ hg book --list-remote 'scratch/*'
     scratch/%erversidebook    0000000000000000000000000000000000000000
     scratch/book              2d9cfa7512136a84a6edb6a7c288145229c2ef7f
  $ hg book --list-remote 'scratch/%*'
     scratch/%erversidebook    0000000000000000000000000000000000000000
#endif
