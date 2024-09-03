# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setconfig push.edenapi=true
  $ ENABLE_API_WRITES=1 setup_common_config "blob_files"
  $ cd $TESTTMP

setup common configuration
  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > ssh="$DUMMYSSH"
  > [extensions]
  > amend=
  > EOF

setup repo
  $ hginit_treemanifest repo
  $ cd repo
  $ hg debugdrawdag <<EOF
  > C
  > |
  > B
  > |
  > A
  > EOF

Clone the repo
  $ cd ..
  $ hg clone -q mono:repo repo2 --noupdate
  $ cd repo2
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > EOF


Modify a file
  $ cd ../repo
  $ hg up -q tip
  $ echo B > A
  $ hg ci -m 'modify copy source'

create master bookmark

  $ hg bookmark master_bookmark -r tip

blobimport them into Mononoke storage and start Mononoke
  $ cd ..
  $ blobimport repo/.hg repo

start mononoke
  $ start_and_wait_for_mononoke_server
Create a copy on a client and push it
  $ cd repo2
  $ hg up -q tip
  $ hg cp A D
  $ hg ci -m 'make a copy'
  $ hg push -r . --to master_bookmark
  pushing rev 726a45528732 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  pushrebasing stack (26805aba1e60, 726a45528732] (1 commit) to remote bookmark master_bookmark
  abort: Server error: Conflicts while pushrebasing: [PushrebaseConflict { left: NonRootMPath("A"), right: NonRootMPath("A") }]
  [255]
