Test the lfs.localstore config option

  $ enable lfs
  $ setconfig lfs.url=file://$TESTTMP/remote lfs.threshold=1

  $ newrepo
  $ drawdag <<'EOS'
  >  C2
  >  |
  >  C1
  > EOS

Both commits create LFS files (flag=2000)

  $ hg debugfilerevision -r 'all()'
  31b5f55a8b18: C1
   C1: bin=0 lnk=0 flag=2000 size=2 copied='' chain=c1f06831b8f9
  a6f371762961: C2
   C2: bin=0 lnk=0 flag=2000 size=2 copied='' chain=cee8fd3a5958

Upload them to remote store

  $ hg debuglfsupload -r 'all()'
  $ cd $TESTTMP
  $ find remote | grep '/../' | sort
  remote/96/85eb765661ea3b95f31e1bb3c3b5501d0c2acdf353feeaa4d8fe32f95f77fb
  remote/ab/861dc170dc2e43224e45278d3d31a675b9ebc34c9b0f48c066ca1eeaed8ee6

  $ cd - &>/dev/null

Remove the local store

  $ rm -rf .hg/store/lfs

Checking out would download LFS files from remote store to local store

  $ hg up -q $C1
  $ find .hg/store/lfs | grep '/../' | sort
  .hg/store/lfs/objects/ab/861dc170dc2e43224e45278d3d31a675b9ebc34c9b0f48c066ca1eeaed8ee6

  $ hg up -q $C2
  $ find .hg/store/lfs | grep '/../' | sort
  .hg/store/lfs/objects/96/85eb765661ea3b95f31e1bb3c3b5501d0c2acdf353feeaa4d8fe32f95f77fb
  .hg/store/lfs/objects/ab/861dc170dc2e43224e45278d3d31a675b9ebc34c9b0f48c066ca1eeaed8ee6

When configured with localstore=false, checking out would skip writing to local store

  $ hg up -q null
  $ rm -rf .hg/store/lfs
  $ setconfig lfs.localstore=false

  $ hg up -q $C1
  $ test -d .hg/store/lfs
  [1]

  $ hg up -q $C2
  $ test -d .hg/store/lfs
  [1]
