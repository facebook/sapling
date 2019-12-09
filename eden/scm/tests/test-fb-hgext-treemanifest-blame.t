#chg-compatible

  $ . "$TESTDIR/library.sh"

  $ cat >> $HGRCPATH <<EOF
  > [treemanifest]
  > sendtrees=True
  > [fastannotate]
  > mainbranch=default
  > modes=fctx
  > EOF

Setup the server

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > fastannotate=
  > [treemanifest]
  > server=True
  > [remotefilelog]
  > server=True
  > shallowtrees=True
  > [fastannotate]
  > server=True
  > EOF

Make local commits on the server
  $ mkdir subdir
  $ echo x >> subdir/x
  $ hg commit -qAm 'add subdir/x'
  $ echo x >> subdir/x
  $ hg commit -qAm 'modify subdir/x'
  $ echo x >> subdir/x
  $ hg commit -qAm 'modify subdir/x 2'
  $ echo x >> subdir/x
  $ hg commit -qAm 'modify subdir/x 3'

Run blame on client
  $ cd ..
  $ hgcloneshallow ssh://user@dummy/master client -q
  fetching tree '' 11a84bc431b8c59b792f10ed427e7a01fc1a6b75, found via 70b8370240f6
  2 trees fetched over * (glob)
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ cd client
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > fastannotate=
  > [treemanifest]
  > demanddownload=True
  > [fastannotate]
  > clientfetchthreshold=2
  > EOF
  $ clearcache
  $ hg prefetch -r 'tip^::tip'
  4 trees fetched over * (glob)
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over * (glob)

- Verify no trees are downloaded
  $ hg blame -r tip -u subdir/x --pager=off
  test: x
  test: x
  test: x
  test: x
