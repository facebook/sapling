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
FIXME: should be contents, not pointer
  $ cat big
  version https://git-lfs.github.com/spec/v1
  oid sha256:e3ade2183b2c023f2f431dacfe0c39617c224c8c21e223ef667b4ee2b3633cb6
  size 10
  x-is-binary 0
