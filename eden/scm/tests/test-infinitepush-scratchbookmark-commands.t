#chg-compatible

  $ setconfig extensions.treemanifest=!
Common configuration for both the server and client.


  $ enable infinitepush remotenames
  $ setconfig infinitepush.branchpattern="re:scratch/.+"
  $ setconfig ui.ssh="python \"$TESTDIR/dummyssh\""


Initialize the server.


  $ newrepo server
  $ setconfig infinitepush.server=yes infinitepush.indextype=disk \
  > infinitepush.storetype=disk phases.new-commit="public"


Make some commits on the server.


  $ echo base > base
  $ hg commit -Aqm "public commit"
  $ echo 1 > file
  $ hg commit -Aqm "another public commit"


Initialize the client.


  $ cd $TESTTMP
  $ hg clone -q ssh://user@dummy/server client
  $ cd client


Push a public commit to a scratch bookmark.


  $ hg push -q --to "scratch/public" --create -r "."


Push a draft commit to a scratch bookmark.


  $ echo 2 > file
  $ hg commit -Aqm "draft commit"
  $ hg push -q --to "scratch/draft" --create -r "."


Test invalid invocations of the `debugcreatescratchbookmark` command.


  $ hg debugcreatescratchbookmark -r "." -B scratch/bookmark
  abort: scratch bookmarks can only be created on an infinitepush server
  [255]

  $ cd ../server
  $ hg debugcreatescratchbookmark -r "all()" -B scratch/bookmark
  abort: must specify exactly one target commit for scratch bookmark
  [255]

  $ hg debugcreatescratchbookmark -r "."
  abort: scratch bookmark name is required
  [255]


Make another public scratch bookmark on an older commit on the server.


  $ hg debugcreatescratchbookmark -r "." -B scratch/anotherpublic


Test that we cannot create a scratch bookmark with the same name.


  $ hg debugcreatescratchbookmark -r "." -B scratch/anotherpublic
  abort: scratch bookmark 'scratch/anotherpublic' already exists
  [255]


Test that we cannot create a real bookmark.


  $ hg debugcreatescratchbookmark -r "." -B nonscratchbookmark
  abort: invalid scratch bookmark name
  [255]


Check that the bookmarks show as expected on the client.


  $ cd ../client
  $ hg log -r "all()" -T '{node|short} "{desc}" {remotebookmarks}\n'
  74903ee2450a "public commit" 
  72feb0cc373f "another public commit" default/scratch/public
  68d8ff913700 "draft commit" default/scratch/draft

  $ hg pull -B scratch/anotherpublic
  pulling from ssh://user@dummy/server
  no changes found

  $ hg log -r "all()" -T '{node|short} "{desc}" {remotebookmarks}\n'
  74903ee2450a "public commit" 
  72feb0cc373f "another public commit" default/scratch/anotherpublic default/scratch/public
  68d8ff913700 "draft commit" default/scratch/draft


Make another scratch bookmark on a draft commit on the server.


  $ cd ../server
  $ hg debugcreatescratchbookmark -r "68d8ff913700" -B  scratch/anotherdraft


Check that the draft scratch bookmark shows up on the client as expected.


  $ cd ../client
  $ hg pull -r scratch/anotherdraft
  pulling from ssh://user@dummy/server
  no changes found
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 1 files

  $ hg log -r "all()" -T '{node|short} "{desc}" {remotebookmarks}\n'
  74903ee2450a "public commit" 
  72feb0cc373f "another public commit" default/scratch/anotherpublic default/scratch/public
  68d8ff913700 "draft commit" default/scratch/anotherdraft default/scratch/draft


Test the attempting to create a scratch bookmark on a non existing commit fails.


  $ cd ../server
  $ hg debugcreatescratchbookmark -r "aaaaaaaaaaaa" -B scratch/bookmark
  abort: unknown revision 'aaaaaaaaaaaa'!
  (if aaaaaaaaaaaa is a remote bookmark or commit, try to 'hg pull' it first)
  [255]


Test invalid invocations of the `debugmovescratchbookmark` command.


  $ cd ../client
  $ hg debugmovescratchbookmark -r "." -B scratch/draft
  abort: scratch bookmarks can only be moved on an infinitepush server
  [255]

  $ cd ../server
  $ hg debugmovescratchbookmark -r "all()" -B scratch/draft
  abort: must specify exactly one target commit for scratch bookmark
  [255]

  $ hg debugmovescratchbookmark -r "."
  abort: scratch bookmark name is required
  [255]

  $ hg debugmovescratchbookmark -r "." -B scratch/nonexistingbookmark
  abort: scratch bookmark 'scratch/nonexistingbookmark' does not exist
  [255]


Test that we cannot move a real bookmark.


  $ hg debugmovescratchbookmark -r "." -B nonscratchbookmark
  abort: invalid scratch bookmark name
  [255]


Move a public scratch bookmark to an older commit on the server.


  $ hg debugmovescratchbookmark -r ".^" -B scratch/public


Check that the bookmarks show as expected on the client.


  $ cd ../client

  $ hg pull -B scratch/public
  pulling from ssh://user@dummy/server
  no changes found

  $ hg log -r "all()" -T '{node|short} "{desc}" {remotebookmarks}\n'
  74903ee2450a "public commit" default/scratch/public
  72feb0cc373f "another public commit" default/scratch/anotherpublic
  68d8ff913700 "draft commit" default/scratch/anotherdraft default/scratch/draft


Push another draft commit to a scratch bookmark.


  $ echo 2 >> file
  $ hg commit -Aqm "another draft commit"
  $ hg push -q --to "scratch/draft" -r "."

  $ hg log -r "all()" -T '{node|short} "{desc}" {remotebookmarks}\n'
  74903ee2450a "public commit" default/scratch/public
  72feb0cc373f "another public commit" default/scratch/anotherpublic
  68d8ff913700 "draft commit" default/scratch/anotherdraft
  6051090c9df8 "another draft commit" default/scratch/draft


Swap the draft scratch bookmarks.


  $ cd ../server
  $ hg debugmovescratchbookmark -r "68d8ff913700" -B scratch/draft
  $ hg debugmovescratchbookmark -r "6051090c9df8" -B scratch/anotherdraft


Check that the bookmarks show as expected on the client.


  $ cd ../client
  $ hg pull -B scratch/draft
  pulling from ssh://user@dummy/server
  no changes found
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 1 files

  $ hg pull -B scratch/anotherdraft
  pulling from ssh://user@dummy/server
  no changes found
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 1 files

  $ hg log -r "all()" -T '{node|short} "{desc}" {remotebookmarks}\n'
  74903ee2450a "public commit" default/scratch/public
  72feb0cc373f "another public commit" default/scratch/anotherpublic
  68d8ff913700 "draft commit" default/scratch/draft
  6051090c9df8 "another draft commit" default/scratch/anotherdraft
