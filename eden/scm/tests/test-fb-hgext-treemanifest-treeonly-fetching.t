#chg-compatible
  $ setconfig workingcopy.ruststatus=False
  $ setconfig experimental.allowfilepeer=True

  $ . "$TESTDIR/library.sh"

Setup the server

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=$TESTDIR/../edenscm/hgext/treemanifestserver.py
  > [treemanifest]
  > server=True
  > treeonly=True
  > [remotefilelog]
  > server=True
  > shallowtrees=True
  > EOF

Setup the client

  $ cd ..
  $ hgcloneshallow ssh://user@dummy/master client -q --config treemanifest.treeonly=True
  $ cd client
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=
  > 
  > [treemanifest]
  > sendtrees=True
  > treeonly=True
  > 
  > [remotefilelog]
  > reponame=treeonlyrepo
  > EOF

Make some commits

  $ cd ../master
  $ mkdir subdir
  $ echo a >> subdir/foo
  $ hg commit -Aqm 'a > subdir/foo'
  $ echo b >> subdir/foo
  $ hg commit -Aqm 'b >> subdir/foo'
  $ echo c >> subdir/foo
  $ hg commit -Aqm 'c >> subdir/foo'
  $ echo d >> subdir/foo
  $ hg commit -Aqm 'd >> subdir/foo'

Test that log -p downloads each tree using the prior tree as a base

  $ cd ../client
  $ hg pull -q
  $ hg up tip
  fetching tree '' f50e2ff15ddef5802543b56b0b84d742512e90f0
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob) (?)
  1 trees fetched over 0.00s
  fetching tree 'subdir' 4b15eef8083c78bd489ff44f06446ebb39d7536e
  1 trees fetched over 0.00s
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg log -p 1>/dev/null
  3 files fetched over 3 fetches - (3 misses, 0.00% hit ratio) over * (glob) (?)
  fetching tree '' 88dd1b582645feb893f44bd3b20947ff2d275360
  1 trees fetched over 0.00s
  fetching tree 'subdir' 38d36c49225fb305cdbeb3524e43df2da3f3c34f
  1 trees fetched over 0.00s
  fetching tree '' d09a7a1172be7d3c00d4bc16831b6394d11ce33f
  1 trees fetched over 0.00s
  fetching tree 'subdir' def6a924bf91641ccf1e45dbabf33ef67dbdf309
  1 trees fetched over 0.00s
  fetching tree '' 1b3e02c1b4460e2d6264781579eb163e76cffad4
  1 trees fetched over 0.00s
  fetching tree 'subdir' 8ab7c090e3a42c4860ca7e141d6b267aa3d31b79
  1 trees fetched over 0.00s
