#chg-compatible
#debugruntest-compatible

  $ configure modernclient
  $ setconfig clone.use-rust=true

  $ newrepo
  $ mv .hg .sl

"root" works in a .sl repo.
  $ hg root
  $TESTTMP/repo1

  $ cd ..


  $ mkdir sapling
  $ cd sapling
Init can create a ".sl" repo.
  $ HGIDENTITY=sl hg init
  $ ls .hg
  $ ls .sl
  00changelog.i
  hgrc.dynamic
  reponame
  requires
  store

  $ cd ..

  $ newremoterepo clone_me
  $ setconfig paths.default=test:clone_me
  $ touch foo
  $ hg commit -Aq -m foo
  $ hg push -r . --to master --create -q

Clone can create a ".sl" repo.
  $ HGIDENTITY=sl hg clone -q test:clone_me cloned
  $ find cloned
  cloned/foo
  $ ls cloned/.hg
  $ ls cloned/.sl
  00changelog.i
  dirstate
  hgrc
  hgrc.dynamic
  reponame
  requires
  store
  treestate
  updateprogress
  wlock.data
  wlock.lock
