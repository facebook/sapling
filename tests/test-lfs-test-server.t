Require lfs-test-server (https://github.com/git-lfs/lfs-test-server)

  $ hash lfs-test-server || { echo 'skipped: missing lfs-test-server'; exit 80; }

  $ LFS_LISTEN="tcp://:$HGPORT"
  $ LFS_HOST="localhost:$HGPORT"
  $ LFS_PUBLIC=1
  $ export LFS_LISTEN LFS_HOST LFS_PUBLIC
  $ lfs-test-server &> lfs-server.log &
  $ echo $! >> $DAEMON_PIDS

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > lfs=$TESTDIR/../hgext3rd/lfs
  > [lfs]
  > url=http://foo:bar@$LFS_HOST/
  > threshold=1
  > EOF

  $ hg init repo1
  $ cd repo1
  $ echo THIS-IS-LFS > a
  $ hg commit -m a -A a

  $ hg init ../repo2
  $ hg push ../repo2 -v
  pushing to ../repo2
  searching for changes
  lfs: computing set of blobs to upload
  lfs: mapping blobs to upload URLs
  lfs: upload completed
  1 changesets found
  uncompressed size of bundle content:
       * (changelog) (glob)
       * (manifests) (glob)
       *  a (glob)
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files

  $ cd ../repo2
  $ hg update tip -v
  resolving manifests
  getting a
  lfs: mapping blobs to download URLs
  lfs: download completed
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Check error message when the remote missed a blob:

  $ echo FFFFF > b
  $ hg commit -m b -A b
  $ echo FFFFF >> b
  $ hg commit -m b b
  $ rm -rf .hg/store/lfs
  $ hg update -C '.^'
  abort: cannot download LFS object 8e6ea5f6c066b44a0efa43bcce86aea73f17e6e23f0663df0251e7524e140a13* (glob)
  [255]
