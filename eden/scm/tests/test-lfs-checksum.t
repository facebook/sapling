#chg-compatible
  $ setconfig experimental.allowfilepeer=True

  $ enable lfs remotefilelog
  $ setconfig lfs.url=file://$TESTTMP/cache lfs.threshold=1 remotefilelog.cachepath=$TESTTMP/rflcache scmstore.enableshim=False

Write a LFS file to the repo

  $ newrepo source
  $ drawdag << 'EOS'
  > A # A/A=LFS
  > EOS

Upload it to the dummy remote store

  $ hg debuglfsupload -r tip

Download it from another repo

  $ newrepo
  $ hg pull ../source -q
  $ hg goto tip -q

Corrupt the remote store

  $ echo corrupted > $TESTTMP/cache/2f/7548e627a92d9ce3f912eb71226f692ec83deed2e72298270b198540d7c70b

Download it again in a fresh new repo - should fail
(using remotefilelog to bypass integrity check at revlog level)

  $ newrepo
  $ echo remotefilelog >> .hg/requires
  $ hg pull ../source -q
  $ hg goto tip -q
  abort: blobstore: sha256 mismatch (oid: 2f7548e627a92d9ce3f912eb71226f692ec83deed2e72298270b198540d7c70b, content: 3dff7d61038895144c0eca9d06ac8d067919785a5ba2604db7aef154899a494d)
  [255]

  $ [ -f A ]
  [1]
