  $ . "${TEST_FIXTURES}/library.sh"

Setup configuration

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > lfs=
  > [lfs]
  > threshold=20B
  > usercache=$TESTTMP/lfs-cache
  > EOF

  $ LFS_THRESHOLD="20" setup_common_config blob_files
  $ REPOID=2 setup_mononoke_repo_config lfs_other
  $ cd "$TESTTMP"

Setup destination repo

  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ echo foo > a
  $ echo foo > b
  $ hg addremove && hg ci -m 'initial'
  adding a
  adding b
  $ enable_replay_verification_hook
  $ cat >> .hg/hgrc <<EOF
  > [treemanifest]
  > treeonly=True
  > EOF
  $ hg bookmark master_bookmark -r tip
  $ cd "$TESTTMP"

Blobimport them into Mononoke storage and start Mononoke
  $ blobimport repo-hg/.hg repo

Start mononoke and a LFS server
  $ mononoke
  $ lfs_base="$(lfs_server)"
  $ lfs_uri="${lfs_base}/repo"
  $ lfs_uri_other="${lfs_base}/lfs_other"
  $ wait_for_mononoke

Make client repo
  $ hgclone_treemanifest ssh://user@dummy/repo-hg client-push --noupdate --config extensions.remotenames= -q
  $ cd client-push

  $ setup_hg_client
  $ setup_hg_lfs "$lfs_uri" 10B "$TESTTMP/lfs-cache1"
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > remotenames =
  > EOF
  $ hg up -q tip

  $ yes A 2>/dev/null | head -c 40 > long
  $ yes B 2>/dev/null | head -c 40 > long2
  $ hg commit -Aqm "add large files"
  $ hgmn push -r . --to master_bookmark -v
  pushing rev 2b6ce7b50f34 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  lfs: need to transfer 2 objects (80 bytes)
  lfs: uploading c12949887b7d8c46e9fcc5d9cd4bd884de33c1d00e24d7ac56ed9200e07f31a1 (40 bytes)
  lfs: processed: c12949887b7d8c46e9fcc5d9cd4bd884de33c1d00e24d7ac56ed9200e07f31a1
  lfs: uploading aac24ec70120b177274d359073212777a40780e2874b120a0f210096e55cfa5f (40 bytes)
  lfs: processed: aac24ec70120b177274d359073212777a40780e2874b120a0f210096e55cfa5f
  validated revset for rebase
  1 changesets found
  uncompressed size of bundle content:
       199 (changelog)
       271  long
       272  long2
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 0 files
  updating bookmark master_bookmark

  $ cd "$TESTTMP"

Two missing blobs: it fails
  $ mononoke_hg_sync repo-hg 1 --generate-bundles --verify-lfs-blob-presence "${lfs_uri_other}/objects/batch" 2>&1 | grep 'objects are missing'
  * caused by: LFS objects are missing: * (glob)

One missing blob: it still fails
  $ hg debuglfssend "$lfs_uri_other" < client-push/long
  c12949887b7d8c46e9fcc5d9cd4bd884de33c1d00e24d7ac56ed9200e07f31a1 40
  $ mononoke_hg_sync repo-hg 1 --generate-bundles --verify-lfs-blob-presence "${lfs_uri_other}/objects/batch" 2>&1 | grep 'objects are missing'
  * caused by: LFS objects are missing: [RequestObject { oid: Sha256(aac24ec70120b177274d359073212777a40780e2874b120a0f210096e55cfa5f), size: 40 }] (glob)

Zero missing blobs: it succeeds
  $ hg debuglfssend "$lfs_uri_other" < client-push/long2
  aac24ec70120b177274d359073212777a40780e2874b120a0f210096e55cfa5f 40
  $ mononoke_hg_sync repo-hg 1 --generate-bundles --verify-lfs-blob-presence "${lfs_uri_other}/objects/batch" 2>&1 | grep 'successful sync'
  * successful sync of entries [2] (glob)
