  $ . "$TESTDIR/library.sh"
  $ setconfig treemanifest.flatcompat=False

Setup the server

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=
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
  fetching tree '' f50e2ff15ddef5802543b56b0b84d742512e90f0, found via 3b158baa90a6
  2 trees fetched over * (glob)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)

  $ hg log -p 1>/dev/null
  fetching tree '' 88dd1b582645feb893f44bd3b20947ff2d275360, based on f50e2ff15ddef5802543b56b0b84d742512e90f0, found via 2f885f603416
  2 trees fetched over * (glob)
  fetching tree '' d09a7a1172be7d3c00d4bc16831b6394d11ce33f, based on 88dd1b582645feb893f44bd3b20947ff2d275360, found via 6bfefea56efe
  2 trees fetched over * (glob)
  fetching tree '' 1b3e02c1b4460e2d6264781579eb163e76cffad4, based on d09a7a1172be7d3c00d4bc16831b6394d11ce33f
  2 trees fetched over * (glob)
  3 files fetched over 3 fetches - (3 misses, 0.00% hit ratio) over * (glob)
