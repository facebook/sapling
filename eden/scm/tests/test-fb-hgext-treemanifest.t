#chg-compatible

  $ . "$TESTDIR/library.sh"


  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=$TESTDIR/../edenscm/hgext/treemanifestserver.py
  > [remotefilelog]
  > server=True
  > [treemanifest]
  > server=True
  > EOF
  $ echo x > x
  $ hg commit -qAm 'add x'
  $ cd ..

  $ hgcloneshallow ssh://user@dummy/master client -q --noupdate
  $ cd client

Test autocreatetrees
  $ cat >> .hg/hgrc <<EOF
  > [treemanifest]
  > autocreatetrees=True
  > EOF
  $ cd ../master
  $ mkdir subdir
  $ echo z >> subdir/z
  $ hg commit -qAm 'add subdir/z'

  $ cd ../client
  $ hg pull
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  $ hg up -r tip
  fetching tree '' 70f2c6726cec346b70b4f2ea65d0e2b9e1092a66, found via e4d61696a942
  2 trees fetched over * (glob)
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over * (glob) (?)
TODO(meyer): Fix debugindexedlogdatastore and debugindexedloghistorystore and add back output here.

Test that commit creates local trees
  $ hg up -q tip
  $ echo z >> subdir/z
  $ hg commit -qAm 'modify subdir/z'
TODO(meyer): Fix debugindexedlogdatastore and debugindexedloghistorystore and add back output here.


Test that manifest matchers work
  $ hg status --rev 1 --rev 2 -I subdir/a
  $ hg status --rev 1 --rev 2 -I subdir/z
  M subdir/z

Test rebasing a stack of commits results in a pack with all the trees

  $ echo >> subdir/y
  $ hg commit -qAm 'modify subdir/y'
  $ echo >> subdir/y
  $ hg commit -Am 'modify subdir/y again'
  $ hg rebase -d 085784c01c08984ae3b6f4e4a6e553035d58380b -s '.^'
  rebasing 6a2476258ba5 "modify subdir/y"
  fetching tree '' bc0c2c938b929f98b1c31a8c5994396ebb096bf0, found via 085784c01c08
  1 trees fetched over * (glob)
  rebasing f096b21e165f "modify subdir/y again"
  $ hg log -r '.^::.' -T '{manifest}\n'
  0d05c20bb7eb53dbfe91f834ed3f0c26ca6ca655
  8289b85c6a307a5a64ffe3bd80bd7998775c787a
TODO(meyer): Fix debugindexedlogdatastore and debugindexedloghistorystore and add back output here.

Test treemanifest with sparse enabled
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > sparse=
  > reset=
  > EOF
  $ hg sparse -I subdir
  $ hg reset '.^'
  1 changeset hidden
  $ hg status
  M subdir/y
  $ hg up -C .
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg sparse --reset

Test rebase two commits with same changes
  $ echo >> subdir/y
  $ hg commit -qm 'modify subdir/y #1'
  $ hg up -q '.^'
  $ echo >> x
  $ echo >> subdir/y
  $ hg commit -qm 'modify subdir/y #2'
  $ hg up -q '.^'
  $ echo >> noop
  $ hg add noop
  $ hg commit -Am 'rebase destination'
  $ hg rebase -d 'desc(rebase)' -r 6052526a0d67 -r 79a69a1547d7 --config rebase.singletransaction=True
  rebasing 6052526a0d67 "modify subdir/y #1"
  rebasing 79a69a1547d7 "modify subdir/y #2"
TODO(meyer): Fix debugindexedlogdatastore and debugindexedloghistorystore and add back output here.
