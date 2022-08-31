#chg-compatible
#debugruntest-compatible

  $ configure modern
  $ setconfig clone.use-rust=true

  $ newrepo
  $ mv .hg .sl

Command doesn't work, but we don't get "not inside a repository error":
  $ LOG=identity=debug hg root
  DEBUG identity: sniffing for repo root start=$TESTTMP/repo1
  DEBUG identity: sniffed repo dir id=sl path=$TESTTMP/repo1
  DEBUG identity: sniffed repo dir id=sl path=$TESTTMP/repo1
  hg: parse errors: required config not found at $TESTTMP/repo1/.hg/hgrc.dynamic
  
  [255]

  $ cd ..


  $ mkdir sapling
  $ cd sapling
Doesn't work yet, but we create a .sl directory.
  $ HGIDENTITY=sl hg init 2>&1 | grep error
  error.RustError: required config not found at $TESTTMP/sapling/.hg/hgrc.dynamic
  $ ls .hg
  $ ls .sl
  00changelog.i
  reponame
  requires
  store

  $ cd ..

  $ newrepo clone_me
  $ touch foo
  $ hg commit -A -m foo -q
  $ cd ..
Doesn't work yet, but tries to create a .sl repo.
  $ HGIDENTITY=sl LOG=identity=debug hg clone eager:clone_me cloned
  DEBUG identity: sniffing for repo root start=$TESTTMP
  Cloning reponame-default into $TESTTMP/cloned
   INFO identity: sniffed identity from env identity="sl"
  DEBUG identity: sniffed repo dir id=sl path=$TESTTMP/cloned
  hg: parse errors: required config not found at $TESTTMP/cloned/.hg/hgrc.dynamic
  
  [255]
