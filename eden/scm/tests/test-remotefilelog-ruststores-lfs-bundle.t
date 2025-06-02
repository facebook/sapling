#require no-eden

  $ setconfig remotefilelog.lfs=True
  $ setconfig lfs.threshold=5
  $ setconfig lfs.url=file:$TESTTMP/lfs-server

  $ newclientrepo

  $ echo "X" > x
  $ hg commit -qAm x
  $ echo "Y" > y
  $ echo "reallybig" > big
  $ hg commit -qAm y

  $ showgraph
  @  5b61e1ea02bb y
  │
  o  766002fed348 x

  $ hg bundle -r tip --base null ~/out.bundle
  2 changesets found

  $ newclientrepo
  $ hg unbundle ~/out.bundle
  adding changesets
  adding manifests
  adding file changes
  $ hg go -q 5b61e1ea02bb
  $ cat big
  reallybig
