# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Setup configuration
  $ INFINITEPUSH_NAMESPACE_REGEX='^scratch/.+$' setup_common_config blob_files
  $ cd "$TESTTMP"

Setup repo
  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ touch a && hg addremove && hg ci -q -ma
  adding a
  $ hg log -T '{short(node)}\n'
  3903775176ed
  $ hg bookmark master_bookmark -r tip

  $ cd "$TESTTMP"
  $ blobimport repo-hg/.hg repo

Start Mononoke
  $ start_and_wait_for_mononoke_server  
  $ lfs_uri="$(lfs_server)/repo"

Setup common client configuration for these tests
  $ cat >> "$HGRCPATH" <<EOF
  > [extensions]
  > amend=
  > infinitepush=
  > commitcloud=
  > remotenames=
  > [infinitepush]
  > server=False
  > branchpattern=re:scratch/.+
  > EOF

setup repo-push and repo-pull
  $ cd "$TESTTMP"
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-push --noupdate
  $ cd "${TESTTMP}/repo-push"
  $ setup_hg_modern_lfs "$lfs_uri" 10B "$TESTTMP/lfs-cache"

  $ cd "$TESTTMP"
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-pull --noupdate
  $ cd "${TESTTMP}/repo-pull"
  $ setup_hg_modern_lfs "$lfs_uri" 10B "$TESTTMP/lfs-cache"

Do infinitepush (aka commit cloud) push
  $ cd "${TESTTMP}/repo-push"
  $ hg up tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo new > newfile
  $ yes A 2>/dev/null | head -c 200 > large
  $ hg addremove -q
  $ hg ci -m new
  $ hgmn push mononoke://$(mononoke_address)/repo -r . --bundle-store --debug --allow-anon
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
  68394cf51f7e96952fe832a3c05d17a9b49e8b4b
  sending unbundle command
  bundle2-output-bundle: "HG20", (1 params) 3 parts total
  bundle2-output-part: "replycaps" * bytes payload (glob)
  bundle2-output-part: "B2X:INFINITEPUSH" (params: 1 advisory) streamed payload
  bundle2-output-part: "b2x:treegroup2" (params: 3 mandatory) streamed payload
  bundle2-input-bundle: 1 params no-transaction
  bundle2-input-part: "reply:changegroup" (params: 2 mandatory) supported
  bundle2-input-bundle: 0 parts total
  preparing listkeys for "bookmarks"
  sending listkeys command
  received listkey for "bookmarks": 57 bytes

Try to pull it
  $ cd "${TESTTMP}/repo-pull"
  $ hgmn pull -r 68394cf51f7e96952fe832a3c05d17a9b49e8b4b
  pulling from mononoke://$LOCALIP:$LOCAL_PORT/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
