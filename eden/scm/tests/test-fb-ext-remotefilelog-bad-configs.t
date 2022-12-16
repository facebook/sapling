#chg-compatible
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

Verify error message when no cachepath specified
  $ hg up -q null
  $ cp $HGRCPATH $HGRCPATH.bak
  $ sed -i.bak -n "/cachepath/!p" $HGRCPATH
  $ hg up tip
  abort: could not find config option remotefilelog.cachepath
  [255]
  $ mv $HGRCPATH.bak $HGRCPATH

Verify error message when no fallback specified

  $ hg up -q null
  $ rm .hg/hgrc
  $ clearcache
  $ hg up tip
  abort: cannot initialize working copy: Missing required config item: edenapi.url
  [255]
