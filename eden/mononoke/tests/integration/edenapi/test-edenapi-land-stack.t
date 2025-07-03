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

Populate test repo
  $ testtool_drawdag -R repo --print-hg-hashes << EOF
  >   H
  >   |
  > E G
  > | |
  > D |
  > | F
  > C |
  >  \|
  >   B
  >   |
  >   A
  > # bookmark: G master_bookmark
  > EOF
  A=20ca2a4749a439b459125ef0f6a4f26e88ee7538
  B=80521a640a0c8f51dcc128c2658b224d595840ac
  C=d3b399ca8757acdb81c3681b052eb978db6768d8
  D=74dbcd84493ad579ee26bb326c4272983098f69c
  E=2576855b2ced4f17d5cf3daa80dd1b9d4b35ddce
  F=b2883237e74ef678eed931f9c9fb1a82ea383597
  G=3e1dde38eb3a3fc75d55232506665e064ef72bfb
  H=c174394d35801d6457160f48133bd63657a6e7bf

Start up SaplingRemoteAPI server.
  $ setup_mononoke_config
  $ SCUBA="$TESTTMP/scuba.json"
  $ start_and_wait_for_mononoke_server --scuba-log-file "$SCUBA"
Clone the repo
  $ cd $TESTTMP
  $ hg clone -q mono:repo repo --noupdate
  $ cd repo
  $ hg pull -q -r $H -r $E

Test land stack
  $ hg debugapi -e landstack -i "'master_bookmark'" -i "'$E'" -i "'$B'"
  {"data": {"Ok": {"new_head": bin("627c455ac6dd21a6528a8977f8f9467f2e83b53e"),
                   "old_to_new_hgids": {bin("2576855b2ced4f17d5cf3daa80dd1b9d4b35ddce"): bin("627c455ac6dd21a6528a8977f8f9467f2e83b53e"),
                                        bin("74dbcd84493ad579ee26bb326c4272983098f69c"): bin("bc6be6ad6dda64396efb330ef20f5edb0dc5ca8b"),
                                        bin("d3b399ca8757acdb81c3681b052eb978db6768d8"): bin("32bf2fcb35e29c21e41dade44d07344e2c54512b")}}}}

Inspect results
  $ hg pull -q
  $ hg log -G -T '{node} {desc} {remotenames}\n' -r "sort(all(),topo)"
  o  627c455ac6dd21a6528a8977f8f9467f2e83b53e E remote/master_bookmark
  │
  o  bc6be6ad6dda64396efb330ef20f5edb0dc5ca8b D
  │
  o  32bf2fcb35e29c21e41dade44d07344e2c54512b C
  │
  │ o  c174394d35801d6457160f48133bd63657a6e7bf H
  ├─╯
  o  3e1dde38eb3a3fc75d55232506665e064ef72bfb G
  │
  o  b2883237e74ef678eed931f9c9fb1a82ea383597 F
  │
  │ o  2576855b2ced4f17d5cf3daa80dd1b9d4b35ddce E
  │ │
  │ o  74dbcd84493ad579ee26bb326c4272983098f69c D
  │ │
  │ o  d3b399ca8757acdb81c3681b052eb978db6768d8 C
  ├─╯
  o  80521a640a0c8f51dcc128c2658b224d595840ac B
  │
  o  20ca2a4749a439b459125ef0f6a4f26e88ee7538 A
  


Test land stack failure - expose server error to client
  $ hg debugapi -e landstack -i "'master_bookmark'" -i "'$C'" -i "'$B'"
  {"data": {"Err": {"code": 0,
                    "message": "Conflicts while pushrebasing: [PushrebaseConflict { left: MPath(\"C\"), right: MPath(\"C\") }]"}}}

  $ cat "$SCUBA" | jq '. | select(.normal.log_tag == "EdenAPI Request Processed" and .normal.edenapi_method == "land_stack") | {edenapi_error: .normal.edenapi_error, edenapi_error_count: .int.edenapi_error_count}'
  {
    "edenapi_error": null,
    "edenapi_error_count": 0
  }
  {
    "edenapi_error": "ServerError { message: \"Conflicts while pushrebasing: [PushrebaseConflict { left: MPath(\\\"C\\\"), right: MPath(\\\"C\\\") }]\", code: 0 }",
    "edenapi_error_count": 1
  }
