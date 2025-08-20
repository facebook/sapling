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

  $ testtool_drawdag -R repo --print-hg-hashes << EOF
  > B
  > |
  > A
  > # bookmark: A alpha
  > # bookmark: B beta
  > EOF
  A=20ca2a4749a439b459125ef0f6a4f26e88ee7538
  B=80521a640a0c8f51dcc128c2658b224d595840ac


Start up SaplingRemoteAPI server.
  $ start_and_wait_for_mononoke_server

Clone repo
  $ hg clone -q mono:repo repo
  $ cd repo

Check response.
  $ hg debugapi -e bookmarks -i '["alpha", "beta", "unknown"]'
  {"beta": "80521a640a0c8f51dcc128c2658b224d595840ac",
   "alpha": "20ca2a4749a439b459125ef0f6a4f26e88ee7538",
   "unknown": None}


Check response for slapigit.
  $ hg --config edenapi.url=https://localhost:$MONONOKE_SOCKET/slapigit/  --config edenapi.ignore-capabilities=true debugapi -e bookmarks -i '["alpha", "beta", "unknown"]'
  {"beta": "be393840a21645c52bbde7e62bdb7269fc3ebb87",
   "alpha": "8131b4f1da6df2caebe93c581ddd303153b338e5",
   "unknown": None}
