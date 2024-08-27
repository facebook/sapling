#require no-eden

  $ setconfig clone.use-rust=true
  $ hg config -q --system -d remotefilelog.reponame

  $ eagerepo
  $ newrepo server
  $ echo "A # bookmark master = A" | drawdag

  $ cd

  $ hg clone eager:$TESTTMP/server client
  Cloning server into $TESTTMP/client (no-windows !)
  Cloning $TESTTMP/server into $TESTTMP\client (windows !)
  Checking out 'master'
  1 files updated
  $ hg -R client config paths.default
  eager:$TESTTMP/server

#if windows
  $ hg clone eager://$TESTTMP/server client2
  Cloning server into $TESTTMP/client2
  abort: $ENOENT$
  [255]
#else
  $ hg clone eager://$TESTTMP/server client2
  Cloning server into $TESTTMP/client2
  Checking out 'master'
  1 files updated
  $ hg -R client2 config paths.default
  eager://$TESTTMP/server
#endif
