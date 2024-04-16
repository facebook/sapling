#debugruntest-compatible

#require no-eden


  $ eagerepo
  $ setconfig remotefilelog.reponame=dont/mess/up
  $ setconfig clone.use-rust=true

TODO(sggutier): figure out why shallow is necessary here (replacing test with eager renders the same results)
  $ hg clone test:dont/mess/up --shallow -q
  $ cd up
  $ hg pull -q
  $ ls $TESTTMP/default-hgcache
  dont%2Fmess%2Fup
