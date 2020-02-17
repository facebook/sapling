#chg-compatible

  $ configure dummyssh
  $ disable treemanifest
  $ enable infinitepush remotenames
  $ setconfig infinitepush.branchpattern="re:scratch/.+"
  $ newrepo server
  $ echo base > base
  $ hg commit -Aqm base
  $ echo 1 > file
  $ hg commit -Aqm commit1
  $ setconfig infinitepush.server=yes infinitepush.indextype=disk infinitepush.storetype=disk
  $ cd $TESTTMP
  $ hg clone ssh://user@dummy/server client1 -q
  $ hg clone ssh://user@dummy/server client2 -q
  $ cd client1

Attempt to push a public commit to a scratch bookmark.  There is no scratch
data to push, but the bookmark should be accepted.

  $ hg push --to scratch/public --create -r . --traceback
  pushing to ssh://user@dummy/server
  searching for changes
  remote: pushing 0 commits:

Pull this bookmark in the other client
  $ cd ../client2
  $ hg up scratch/public
  'scratch/public' does not exist locally - looking for it remotely...
  pulling from ssh://user@dummy/server
  no changes found
  'scratch/public' found remotely
  pull finished in * (glob)
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -r . -T '{node|short} "{desc}" {remotebookmarks}\n'
  e6c779c67aa9 "commit1" default/scratch/public
  $ cd ../client1

Attempt to push a public commit to a real remote bookmark.  This should also
be accepted.

  $ hg push --to real-public --create -r .
  pushing rev e6c779c67aa9 to destination ssh://user@dummy/server bookmark real-public
  searching for changes
  no changes found
  exporting bookmark real-public
  [1]

Attempt to push a draft commit to a scratch bookmark.  This should still work.

  $ echo 2 > file
  $ hg commit -Aqm commit2
  $ hg push --to scratch/draft --create -r .
  pushing to ssh://user@dummy/server
  searching for changes
  remote: pushing 1 commit:
  remote:     3f2e32144a89  commit2

Check the server data is correct.

  $ cat $TESTTMP/server/.hg/scratchbranches/index/bookmarkmap/scratch/public
  e6c779c67aa947c951f334f4f312bd2b21d27e55 (no-eol)
  $ cat $TESTTMP/server/.hg/scratchbranches/index/bookmarkmap/scratch/draft
  3f2e32144a89cb84ece9ddd3ec1ac2ddf440d113 (no-eol)
  $ hg bookmarks --cwd $TESTTMP/server
     real-public               1:e6c779c67aa9

Make another public scratch bookmark on an older commit.

  $ hg up -q 0
  $ hg push --to scratch/other --create -r .
  pushing to ssh://user@dummy/server
  searching for changes
  remote: pushing 0 commits:

  $ cat $TESTTMP/server/.hg/scratchbranches/index/bookmarkmap/scratch/other
  d20a80d4def38df63a4b330b7fb688f3d4cae1e3 (no-eol)

Make a new draft commit here, and push it to the other scratch bookmark.  This
works because the old commit is an ancestor of the new commit.

  $ echo a > other
  $ hg commit -Aqm other1
  $ hg push --to scratch/other -r .
  pushing to ssh://user@dummy/server
  searching for changes
  remote: pushing 1 commit:
  remote:     8bebbb8c3ae7  other1

  $ cat $TESTTMP/server/.hg/scratchbranches/index/bookmarkmap/scratch/other
  8bebbb8c3ae7d0404be1a13386747db4bc43806e (no-eol)

Push the draft commit onto the original public scratch bookmark.  It should
fail because the bookmark is not an ancestor of this commit.

  $ hg push --to scratch/public -r .
  pushing to ssh://user@dummy/server
  searching for changes
  remote: non-forward push
  remote: (use --non-forward-move to override)
  abort: push failed on remote
  [255]

  $ cat $TESTTMP/server/.hg/scratchbranches/index/bookmarkmap/scratch/public
  e6c779c67aa947c951f334f4f312bd2b21d27e55 (no-eol)

Try again with --non-forward-move.

  $ hg push --to scratch/public --non-forward-move -r .
  pushing to ssh://user@dummy/server
  searching for changes
  remote: pushing 1 commit:
  remote:     8bebbb8c3ae7  other1

  $ cat $TESTTMP/server/.hg/scratchbranches/index/bookmarkmap/scratch/public
  8bebbb8c3ae7d0404be1a13386747db4bc43806e (no-eol)

Move the two bookmarks back to a public commit.

  $ hg push --to scratch/public --non-forward-move -r 0
  pushing to ssh://user@dummy/server
  searching for changes
  remote: pushing 0 commits:
  $ hg push --to scratch/other --non-forward-move -r 1
  pushing to ssh://user@dummy/server
  searching for changes
  remote: pushing 0 commits:

Update the public scratch bookmarks in the other client, using both -r and -B.

  $ cd ../client2
  $ hg log -r scratch/public -T '{node|short} "{desc}" {remotebookmarks}\n'
  e6c779c67aa9 "commit1" default/scratch/public
  $ hg pull -r scratch/public
  pulling from ssh://user@dummy/server
  no changes found
  $ hg log -r scratch/public -T '{node|short} "{desc}" {remotebookmarks}\n'
  d20a80d4def3 "base" default/scratch/public
  $ hg pull -B scratch/other
  pulling from ssh://user@dummy/server
  no changes found
  $ hg log -r scratch/other -T '{node|short} "{desc}" {remotebookmarks}\n'
  e6c779c67aa9 "commit1" default/real-public default/scratch/other
