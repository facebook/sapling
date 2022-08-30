#chg-compatible
#debugruntest-compatible

  $ configure modern

  $ newrepo
  $ mv .hg .sl

Command doesn't work, but we don't get "not inside a repository error":
  $ export LOG=identity=debug
  $ hg root
  DEBUG identity: sniffing for repo root start=$TESTTMP/repo1
  DEBUG identity: sniffed repo dir id=sl path=$TESTTMP/repo1
  DEBUG identity: sniffed repo dir id=sl path=$TESTTMP/repo1
  hg: parse errors: required config not found at "$TESTTMP/repo1/.hg/hgrc.dynamic"
  
  [255]
