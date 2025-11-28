# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration

  $ setup_common_config "blob_files"
  $ cd $TESTTMP

Setup repo

  $ testtool_drawdag -R repo <<EOF
  > A
  > # modify: A a "a file content"
  > # bookmark: A master_bookmark
  > EOF
  A=d672564be4c568b4d175fb2283de2485ea31cbe1d632ff2a6850b69e2940bad8

Start mononoke and the LFS Server

  $ start_and_wait_for_mononoke_server
  $ lfs_uri="$(lfs_server)/repo"

Setup client repo

  $ hg clone -q mono:repo hg-client
  $ cd hg-client
  $ setup_hg_modern_lfs "$lfs_uri" 10B "$TESTTMP/lfs-cache"

Create new commits

  $ mkdir b_dir
  $ hg up master_bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo "regular file" > small
  $ yes A 2>/dev/null | head -c 200 > large
  $ hg commit -Aqm "add files"
  $ hg push --debug --to master_bookmark
  sending hello command
  sending clienttelemetry command
  pushing rev 8e48c8a863b5 to destination mono:repo bookmark master_bookmark
  query 1; heads
  searching for changes
  local heads: 1; remote heads: 1 (explicit: 0); initial common: 1
  1 total queries in 0.0000s
  preparing listkeys for "bookmarks" with pattern "['master_bookmark']"
  sending listkeyspatterns command
  received listkey for "bookmarks": 56 bytes
  1 changesets found
  list of changesets:
  8e48c8a863b58f9eddc3b3d152801cb45a81dfd4
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
  preparing listkeys for "bookmarks" with pattern "['master_bookmark']"
  sending listkeyspatterns command
  received listkey for "bookmarks": 56 bytes

Clone the repository, and pull

  $ hg clone -q mono:repo hg-client
  $ cd hg-client
  $ setup_hg_modern_lfs "$lfs_uri" 10B "$TESTTMP/lfs-cache"
  $ hg pull -q
  $ hg up -q master_bookmark
  $ sha256sum large
  f9f7889fcedc8580403673810e2be90e35980f10234f80d08a6497bbda16a245  large
