#chg-compatible
  $ configure modernclient

no-check-code
  $ . "$TESTDIR/library.sh"

  $ newclientrepo master
  $ echo x > x
  $ echo y > y
  $ echo z > z
  $ sl commit -qAm xy
  $ sl push --to master --create -q

  $ newclientrepo shallow master_server

Verify error message when no fallback specified

  $ sl up -q null
  $ rm .sl/config
  $ clearcache
  $ sl up tip
  abort: *The commit graph requires a remote peer but the repo does not have one* (glob)
  [255]
