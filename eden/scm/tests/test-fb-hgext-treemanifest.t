#chg-compatible
  $ setconfig experimental.allowfilepeer=True

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
  $ hg up -r tip
  fetching tree '' 3171d1d9315ec883e4028e787f617120bd06cfa8
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over * (glob) (?)
  1 trees fetched over 0.00s
  fetching tree 'subdir' ddb35f099a648a43a997aef53123bce309c794fd
  1 trees fetched over 0.00s
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
TODO(meyer): Fix debugindexedlogdatastore and debugindexedloghistorystore and add back output here.

Test that commit creates local trees
  $ hg up -q tip
  $ echo z >> subdir/z
  $ hg commit -qAm 'modify subdir/z'
TODO(meyer): Fix debugindexedlogdatastore and debugindexedloghistorystore and add back output here.


Test that manifest matchers work
  $ hg status --rev 'desc("add subdir/z")' --rev 'desc("modify subdir/z")' -I subdir/a
  $ hg status --rev 'desc("add subdir/z")' --rev 'desc("modify subdir/z")' -I subdir/z
  M subdir/z

Test rebasing a stack of commits results in a pack with all the trees

  $ echo >> subdir/y
  $ hg commit -qAm 'modify subdir/y'
  $ echo >> subdir/y
  $ hg commit -Am 'modify subdir/y again'
  $ hg rebase -d 085784c01c08984ae3b6f4e4a6e553035d58380b -s '.^'
  rebasing * "modify subdir/y" (glob)
  fetching tree '' bc0c2c938b929f98b1c31a8c5994396ebb096bf0
  1 trees fetched over * (glob)
  rebasing * "modify subdir/y again" (glob)
  $ hg log -r '.^::.' -T '{manifest}\n'
  0e5087e257eeb8a1418a1ec5f4395fb17b8c1b4f
  ba4fcc53f7c9ac6201325aed3e64b83905bd5784
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
  $ hg rebase -d 'desc(rebase)' -r 'desc("#1")' -r 'desc("#2")' --config rebase.singletransaction=True
  rebasing * "modify subdir/y #1" (glob)
  rebasing * "modify subdir/y #2" (glob)
TODO(meyer): Fix debugindexedlogdatastore and debugindexedloghistorystore and add back output here.
