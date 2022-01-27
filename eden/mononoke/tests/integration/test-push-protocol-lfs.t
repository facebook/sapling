# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration

  $ setup_common_config "blob_files"
  $ cd $TESTTMP

Setup repo and blobimport it

  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ echo "a file content" > a
  $ hg add a
  $ hg ci -ma
  $ hg bookmark master_bookmark -r 'tip'
  $ cd "$TESTTMP"
  $ blobimport repo-hg/.hg repo

Start mononoke and the LFS Server

  $ start_and_wait_for_mononoke_server
  $ lfs_uri="$(lfs_server)/repo"

Setup client repo

  $ hgclone_treemanifest ssh://user@dummy/repo-hg hg-client
  $ cd hg-client
  $ setup_hg_modern_lfs "$lfs_uri" 10B "$TESTTMP/lfs-cache"

Create new commits

  $ mkdir b_dir
  $ hg up master_bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark master_bookmark)
  $ echo "regular file" > small
  $ yes A 2>/dev/null | head -c 200 > large
  $ hg commit -Aqm "add files"
  $ hgmn push --debug
  pushing to mononoke://$LOCALIP:$LOCAL_PORT/repo
  sending hello command
  sending clienttelemetry command
  query 1; heads
  sending batch command
  searching for changes
  local heads: 1; remote heads: 1 (explicit: 0); initial common: 1
  1 total queries in 0.0000s
  checking for updated bookmarks
  preparing listkeys for "bookmarks"
  sending listkeys command
  received listkey for "bookmarks": 57 bytes
  1 changesets found
  list of changesets:
  48d4d2fa17e54179e24de7fcb4a8ced38738ca4e
  sending unbundle command
  bundle2-output-bundle: "HG20", 4 parts total
  bundle2-output-part: "replycaps" 219 bytes payload
  bundle2-output-part: "changegroup" (params: 1 mandatory) streamed payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-output-part: "b2x:treegroup2" (params: 3 mandatory) streamed payload
  bundle2-input-bundle: 1 params no-transaction
  bundle2-input-part: "reply:changegroup" (params: 2 mandatory) supported
  bundle2-input-part: "reply:pushkey" (params: 2 mandatory) supported
  bundle2-input-bundle: 1 parts total
  updating bookmark master_bookmark

Clone the repository, and pull

  $ hgclone_treemanifest ssh://user@dummy/repo-hg hg-client
  $ cd hg-client
  $ setup_hg_modern_lfs "$lfs_uri" 10B "$TESTTMP/lfs-cache"
  $ hgmn pull
  pulling from mononoke://$LOCALIP:$LOCAL_PORT/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark
  $ hgmn up master_bookmark
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark master_bookmark)
  $ sha256sum large
  f9f7889fcedc8580403673810e2be90e35980f10234f80d08a6497bbda16a245  large
