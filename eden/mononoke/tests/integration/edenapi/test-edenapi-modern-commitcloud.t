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
  $ mkcommit "base_commit"
  $ hg log -T '{node}\n'
  8b2dca0c8a726d66bf26d47835a356cc4286facd
  $ hg bookmark master -r tip

Start up SaplingRemoteAPI server.
  $ setup_mononoke_config
  $ start_and_wait_for_mononoke_server

Check response.
  $ hgedenapi debugapi -e cloudupdatereferences -i "{'workspace':'user/integrationtest/default','reponame':'repo','version':0, 'removed_heads':[], 'new_heads':[ '8b2dca0c8a726d66bf26d47835a356cc4286facd'], 'updated_bookmarks':[('master', '8b2dca0c8a726d66bf26d47835a356cc4286facd')], 'removed_bookmarks':[], 'new_snapshots':[], 'removed_snapshots':[]}"
  {"heads": None,
   "version": 1,
   "bookmarks": None,
   "snapshots": None,
   "timestamp": *, (glob)
   "heads_dates": None,
   "remote_bookmarks": None}

  $ hgedenapi debugapi -e cloudreferences -i "{'workspace':'user/integrationtest/default','reponame':'repo','version':0}"
  {"heads": [bin("8b2dca0c8a726d66bf26d47835a356cc4286facd")],
   "version": 1,
   "bookmarks": {"master": bin("8b2dca0c8a726d66bf26d47835a356cc4286facd")},
   "snapshots": [],
   "timestamp": *, (glob)
   "heads_dates": {bin("8b2dca0c8a726d66bf26d47835a356cc4286facd"): *}, (glob)
   "remote_bookmarks": []}

  $ hgedenapi debugapi -e cloudworkspace -i "'user/integrationtest/default'" -i "'repo'"
  {"name": "user/integrationtest/default",
   "version": 1,
   "archived": False,
   "reponame": "repo",
   "timestamp": *} (glob)
