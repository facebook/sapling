# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

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
  $ setup_hg_modern_lfs "$lfs_uri" 10B "$TESTTMP/lfs-cache1"
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
  pushing rev 2b6ce7b50f34 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  validated revset for rebase
  1 changesets found
  uncompressed size of bundle content:
       199 (changelog)
       271  long
       272  long2
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

  $ cd "$TESTTMP"

Two missing blobs that were uploaded
  $ mononoke_hg_sync repo-hg 1 --verify-lfs-blob-presence "${lfs_uri_other}/objects/batch" 2>&1 | grep missing
  * missing * object, uploading* (glob)
  * missing * object, uploading* (glob)

Check that they were uploaded
  $ hg debuglfsreceive c12949887b7d8c46e9fcc5d9cd4bd884de33c1d00e24d7ac56ed9200e07f31a1 0 "${lfs_uri_other}" > "$TESTTMP/long"
  $ cmp "$TESTTMP/long" client-push/long

  $ hg debuglfsreceive aac24ec70120b177274d359073212777a40780e2874b120a0f210096e55cfa5f 0 "${lfs_uri_other}" > "$TESTTMP/long2"
  $ cmp "$TESTTMP/long2" client-push/long2
