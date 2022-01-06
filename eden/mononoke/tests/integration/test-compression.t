# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ MONONOKE_DIRECT_PEER=1
  $ setup_common_config
  $ cd $TESTTMP

setup repo with 1MB file, which is larger then zstd stream buffer size
  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ printf '=%.0s' {1..1048576} > a
  $ hg add a
  $ hg ci -ma

setup master bookmarks
  $ hg bookmark master_bookmark -r 'tip'
  $ cd $TESTTMP
  $ blobimport repo-hg/.hg repo
  $ rm -rf repo-hg

Setup the right configuration
  $ merge_tunables <<EOF
  > {
  >   "ints": {
  >     "zstd_compression_level": 3
  >   }
  > }
  > EOF

start mononoke
  $ mononoke
  $ wait_for_mononoke

clone and checkout the repository with compression enabled
  $ hg clone -U --shallow --debug "mononoke://$(mononoke_address)/repo" --config mononokepeer.compression=true 2>&1 | grep zstd
  zstd compression on the wire is enabled
  $ cd repo
  $ hgmn checkout master_bookmark --config mononokepeer.compression=true
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark master_bookmark)

without compression again, no zstd indicator that compression is used
  $ hgmn pull --debug 2>&1 | grep -P "(zstd|pulling|checking)"
  pulling from mononoke://* (glob)
  checking for updated bookmarks
