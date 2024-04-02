#debugruntest-compatible

#require no-eden


  $ eagerepo
  $ setconfig remotefilelog.reponame=dont/mess/up
  $ setconfig clone.use-rust=true

  $ hg clone -q test:dont/mess/up
  $ cd up
  $ hg pull -q
  $ ls $TESTTMP/default-hgcache
  dont%2Fmess%2Fup
