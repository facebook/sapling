# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Set up local hgrc and Mononoke config.
  $ setup_common_config
  $ setup_configerator_configs
  $ cd $TESTTMP

Initialize test repo.
  $ hginit_treemanifest repo
  $ cd repo
  $ drawdag << EOF
  > B
  > |
  > A
  > EOF

import testing repo
  $ cd ..
  $ blobimport repo/.hg repo

Start up SaplingRemoteAPI server.
  $ setup_mononoke_config
  $ start_and_wait_for_mononoke_server
Check response.
  $ hg debugapi mono:repo -e ephemeralprepare -i None -i "['some', 'label']"
  {"bubble_id": 1}
