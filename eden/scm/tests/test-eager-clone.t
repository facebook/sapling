  $ setconfig clone.use-rust=true
  $ hg config -q --system -d remotefilelog.reponame

  $ eagerepo
  $ newrepo server
  $ echo "A # bookmark master = A" | drawdag

  $ cd

  $ hg clone eager:$TESTTMP/server client
  Cloning server into $TESTTMP/client
  Checking out 'master' (no-eden !)
  1 files updated (no-eden !)
  $ hg -R client config paths.default
  eager:$TESTTMP/server

#if eden
  $ setconfig edenfs.backing-repos-dir=$TESTTMP/.eden-backing-repos2
#endif

  $ hg clone eager://$TESTTMP/server client2
  Cloning server into $TESTTMP/client2
  Checking out 'master' (no-eden !)
  1 files updated (no-eden !)
  $ hg -R client2 config paths.default
  eager://$TESTTMP/server
