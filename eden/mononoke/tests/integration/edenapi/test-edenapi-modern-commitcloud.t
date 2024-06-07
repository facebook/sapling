# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration

  $ REPOID=0 REPONAME=repo setup_common_config blob_files

setup repo
  $ testtool_drawdag -R repo --derive-all << EOF
  > A-B-C
  > # bookmark: A main
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ start_and_wait_for_mononoke_server 
  $ hgedenapi clone -q  "mononoke://$(mononoke_address)/repo" client1
  $ cd client1
  $ hg -q co main
  $ hg log -T "{node}"
  20ca2a4749a439b459125ef0f6a4f26e88ee7538 (no-eol)
  $ mononoke_newadmin convert -R repo --from hg --to bonsai 20ca2a4749a439b459125ef0f6a4f26e88ee7538
  aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675


Check response.
  $ hgedenapi debugapi -e cloudupdatereferences -i "{'workspace':'user/integrationtest/default','reponame':'repo','version':0, 'removed_heads':[], 'new_heads':[ '20ca2a4749a439b459125ef0f6a4f26e88ee7538'], 'updated_bookmarks':[('main', '20ca2a4749a439b459125ef0f6a4f26e88ee7538')], 'removed_bookmarks':[], 'new_snapshots':[], 'removed_snapshots':[]}"
  {"heads": None,
   "version": 1,
   "bookmarks": None,
   "snapshots": None,
   "timestamp": *, (glob)
   "heads_dates": None,
   "remote_bookmarks": None}

  $ hgedenapi debugapi -e cloudreferences -i "{'workspace':'user/integrationtest/default','reponame':'repo','version':0}"
  {"heads": [bin("20ca2a4749a439b459125ef0f6a4f26e88ee7538")],
   "version": 1,
   "bookmarks": {"main": bin("20ca2a4749a439b459125ef0f6a4f26e88ee7538")},
   "snapshots": [],
   "timestamp": *, (glob)
   "heads_dates": {bin("20ca2a4749a439b459125ef0f6a4f26e88ee7538"): 0},
   "remote_bookmarks": []}
  $ hgedenapi debugapi -e cloudworkspace -i "'user/integrationtest/default'" -i "'repo'"
  {"name": "user/integrationtest/default",
   "version": 1,
   "archived": False,
   "reponame": "repo",
   "timestamp": *} (glob)
