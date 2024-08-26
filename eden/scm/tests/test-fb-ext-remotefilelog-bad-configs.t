#chg-compatible
#debugruntest-incompatible
  $ configure modernclient

no-check-code
  $ . "$TESTDIR/library.sh"

  $ newclientrepo master
  $ echo x > x
  $ echo y > y
  $ echo z > z
  $ hg commit -qAm xy
  $ hg push --to master --create -q

  $ newclientrepo shallow test:master_server

Verify error message when no fallback specified

  $ hg up -q null
  $ rm .hg/hgrc
  $ clearcache
  $ hg up tip
  abort: *The commit graph requires a remote peer but the repo does not have one* (glob)
  [255]
