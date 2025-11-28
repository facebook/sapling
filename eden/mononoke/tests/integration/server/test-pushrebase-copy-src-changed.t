# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setconfig push.edenapi=true
  $ setup_common_config "blob_files"
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
  $ testtool_drawdag -R repo --no-default-files <<EOF
  > A-B-C-D
  > # modify: A "A" "A\n"
  > # modify: B "B" "B\n"
  > # modify: C "C" "C\n"
  > # modify: D "A" "B\n"
  > # bookmark: D master_bookmark
  > EOF
  A=f8e66e754c1ddb2ec3372b9614905e2bca804e5ef516750aa2ca83acbfa76942
  B=4b83a91fb7682abf7d84c83fb4711d0ee4a1b45d8c174f576cb46fb9cda42833
  C=dca943b3076fff2c3621c135106ffd6a3dd411a63a249e8d4e6b159a2b2ee829
  D=1bc7e0d047ac1a47d6f83ee794994ca900aae79ed55dd035f7bdfed7dde4fa3d

Import and start mononoke
  $ cd "$TESTTMP"
  $ mononoke
  $ wait_for_mononoke

Clone the repo
  $ hg clone -q mono:repo repo2 --noupdate
  $ cd repo2
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > EOF

Create a copy on a client and push it
Get the C commit hash to update to the commit before D
  $ C=$(hg log -r 'master_bookmark~1' -T '{node}')
  $ hg up -q $C
  $ hg cp A D
  $ hg ci -m 'make a copy'
  $ hg push -r . --to master_bookmark
  pushing rev f2179094725c to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  pushrebasing stack (8ce11a1536a9, f2179094725c] (1 commit) to remote bookmark master_bookmark
  abort: Server error: Conflicts while pushrebasing: [PushrebaseConflict { left: MPath("A"), right: MPath("A") }]
  [255]
