#chg-compatible

Inital setup

  $ disable treemanifest
  $ . "$TESTDIR/hgsql/library.sh"
  $ initclient client
  $ initserver server lfsrepo

  $ mkdir -p $TESTTMP/lfs-remote

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > lfs=
  > [lfs]
  > url=file://$TESTTMP/lfs-remote
  > EOF

  $ cd client
  $ cat >> .hg/hgrc << EOF
  > [lfs]
  > threshold=10B
  > chunksize=10B
  > EOF

Commit some large file stuff

  $ echo LONGER-THAN-TEN-BYTES-WILL-TRIGGER-LFS > large
  $ echo SHORTER > small
  $ hg add . -q
  $ hg commit -m 'commit with lfs content'

  $ hg mv large l
  $ hg mv small s
  $ hg commit -m 'renames'

  $ echo SHORT > l
  $ echo BECOME-LARGER-FROM-SHORTER > s
  $ hg commit -m 'large to small, small to large'

  $ echo 1 >> l
  $ echo 2 >> s
  $ hg commit -m 'random modifications'

  $ echo RESTORE-TO-BE-LARGE > l
  $ echo SHORTER > s
  $ hg commit -m 'switch large and small again'

  $ hg bookmark @
  $ hg push ../server/ -q --trace
  lfs: * (glob) (?)
  lfs: * (glob) (?)

  $ cd ..

Clone the repo from SQL

  $ initserver server2 lfsrepo

Verify all repos

  $ for r in client server server2; do
  >   echo repo: $r
  >   hg --cwd $TESTTMP/$r verify
  > done
  repo: client
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  4 files, 5 changesets, 10 total revisions
  repo: server
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  4 files, 5 changesets, 10 total revisions
  repo: server2
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  4 files, 5 changesets, 10 total revisions
