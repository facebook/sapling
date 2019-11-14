  $ setconfig extensions.treemanifest=!
  $ . "$TESTDIR/library.sh"
  $ setconfig treemanifest.treeonly=False

Start with a server that has flat manifests

  $ newrepo master
  $ enable infinitepush
  $ setconfig remotefilelog.server=true infinitepush.server=true
  $ setconfig infinitepush.branchpattern=re:scratch/.+
  $ setconfig infinitepush.indextype=disk infinitepush.storetype=disk
  $ mkdir dir1
  $ echo base > dir1/base
  $ hg commit -Aqm base

Make a remotefilelog client

  $ cd $TESTTMP
  $ hgcloneshallow ssh://user@dummy/master client1
  streaming all changes
  3 files to transfer, * of data (glob)
  transferred * (glob)
  searching for changes
  no changes found
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 files fetched over 1 fetches - * (glob)
  $ cd client1
  $ enable infinitepush
  $ setconfig infinitepush.server=false infinitepush.branchpattern=re:scratch/.+

Push a bundle with four commits

  $ mkdir dir2
  $ echo 1 > dir2/bundled
  $ hg commit -Aqm bundled1
  $ echo 2 > dir2/bundled
  $ hg commit -Aqm bundled2
  $ echo 3 > dir2/bundled
  $ hg commit -Aqm bundled3
  $ echo 4 > dir2/bundled
  $ hg commit -Aqm bundled4
  $ tglog
  @  4: d1944cedf06c 'bundled4'
  |
  o  3: 916baec915e2 'bundled3'
  |
  o  2: 9494660bae92 'bundled2'
  |
  o  1: f570e0648bfb 'bundled1'
  |
  o  0: f7e449aab27f 'base'
  

  $ hg push -r . --to scratch/bundled --create
  pushing to ssh://user@dummy/master
  searching for changes
  remote: pushing 4 commits:
  remote:     f570e0648bfb  bundled1
  remote:     9494660bae92  bundled2
  remote:     916baec915e2  bundled3
  remote:     d1944cedf06c  bundled4

Upgrade the server to treemanifest

  $ cd $TESTTMP/master
  $ enable treemanifest
  $ setconfig treemanifest.server=true
  $ setconfig fastmanifest.usetree=true fastmanifest.usecache=false

  $ hg backfilltree

  $ setconfig treemanifest.treeonly=true

Clone another client, this time treeonly

  $ cd $TESTTMP
  $ hgcloneshallow ssh://user@dummy/master client2 --config extensions.treemanifest= --config treemanifest.treeonly=true
  streaming all changes
  3 files to transfer, * of data (glob)
  transferred * (glob)
  searching for changes
  no changes found
  updating to branch default
  fetching tree '' a8b0ba84fc9d10d4e1e5be15a0f2b83872021770
  2 trees fetched over * (glob)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd client2
  $ enable infinitepush
  $ enable treemanifest
  $ setconfig fastmanifest.usetree=true fastmanifest.usecache=false
  $ setconfig treemanifest.treeonly=true

Pull three of the commits, triggering a rebundle.  The server must include all of the
trees for the infinitepush commits.

  $ hg pull -r 916baec915e2
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 1 files
  new changesets f570e0648bfb:916baec915e2

Make sure we can check out the commit we pulled

  $ hg update 916baec915e2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
