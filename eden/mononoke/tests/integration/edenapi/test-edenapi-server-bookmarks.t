# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Set up local hgrc and Mononoke config.
  $ setup_common_config
  $ cd $TESTTMP


Setup testing repo for mononoke:
  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ setup_hg_server


  $ drawdag << EOS
  > B
  > |
  > A
  > EOS

  $ hg book -r $A alpha
  $ hg log -r alpha -T'{node}\n'
  426bada5c67598ca65036d57d9e4b64b0c1ce7a0
  $ hg book -r $B beta
  $ hg log -r beta -T'{node}\n'
  112478962961147124edd43549aedd1a335e44bf


import testing repo to mononoke
  $ cd ..
  $ blobimport repo-hg/.hg repo


Start up EdenAPI server.
  $ start_and_wait_for_mononoke_server
Check response.
  $ hgedenapi debugapi -e bookmarks -i '["alpha", "beta", "unknown"]'
  {"beta": "112478962961147124edd43549aedd1a335e44bf",
   "alpha": "426bada5c67598ca65036d57d9e4b64b0c1ce7a0",
   "unknown": None}
