# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Set up local hgrc and Mononoke config.
  $ setup_common_config
  $ setup_configerator_configs
  $ setconfig remotenames.selectivepulldefault=master_bookmark,to_delete,create_bookmark
  $ cd $TESTTMP


Populate test repo
  $ testtool_drawdag -R repo  --print-hg-hashes << EOF
  >   E
  >   |
  >   D
  >   |
  >   C
  >   |
  >   B
  >   |
  >   A
  > # bookmark: C master_bookmark
  > # bookmark: E to_delete
  > EOF
  A=20ca2a4749a439b459125ef0f6a4f26e88ee7538
  B=80521a640a0c8f51dcc128c2658b224d595840ac
  C=d3b399ca8757acdb81c3681b052eb978db6768d8
  D=74dbcd84493ad579ee26bb326c4272983098f69c
  E=2576855b2ced4f17d5cf3daa80dd1b9d4b35ddce

Start up SaplingRemoteAPI server.
  $ setup_mononoke_config
  $ start_and_wait_for_mononoke_server
Clone the repo
  $ cd $TESTTMP
  $ hg clone -q mono:repo repo2 --noupdate
  $ cd repo2

Test move bookmark
  $ hg debugapi -e setbookmark -i "'master_bookmark'" -i "'$E'" -i "'$C'"
  {"data": {"Ok": None}}

Inspect results
  $ hg pull -q
  $ hg log -G -T '{node} {desc} {remotenames}\n' -r "all()"
  o  2576855b2ced4f17d5cf3daa80dd1b9d4b35ddce E remote/master_bookmark remote/to_delete
  │
  o  74dbcd84493ad579ee26bb326c4272983098f69c D
  │
  o  d3b399ca8757acdb81c3681b052eb978db6768d8 C
  │
  o  80521a640a0c8f51dcc128c2658b224d595840ac B
  │
  o  20ca2a4749a439b459125ef0f6a4f26e88ee7538 A
  


Test delete bookmark
  $ hg debugapi -e setbookmark -i "'to_delete'" -i "None" -i "'$E'"
  {"data": {"Ok": None}}

Inspect results
  $ hg pull -q
  $ hg log -G -T '{node} {desc} {remotenames}\n' -r "all()"
  o  2576855b2ced4f17d5cf3daa80dd1b9d4b35ddce E remote/master_bookmark
  │
  o  74dbcd84493ad579ee26bb326c4272983098f69c D
  │
  o  d3b399ca8757acdb81c3681b052eb978db6768d8 C
  │
  o  80521a640a0c8f51dcc128c2658b224d595840ac B
  │
  o  20ca2a4749a439b459125ef0f6a4f26e88ee7538 A
  


Test create bookmark
  $ hg debugapi -e setbookmark -i "'create_bookmark'" -i "'$B'" -i "None"
  {"data": {"Ok": None}}

Inspect results
  $ hg pull -q
  $ hg log -G -T '{node} {desc} {remotenames}\n' -r "all()"
  o  2576855b2ced4f17d5cf3daa80dd1b9d4b35ddce E remote/master_bookmark
  │
  o  74dbcd84493ad579ee26bb326c4272983098f69c D
  │
  o  d3b399ca8757acdb81c3681b052eb978db6768d8 C
  │
  o  80521a640a0c8f51dcc128c2658b224d595840ac B remote/create_bookmark
  │
  o  20ca2a4749a439b459125ef0f6a4f26e88ee7538 A
  


Test bookmark failure (empty from and to)
  $ hg debugapi -e setbookmark -i "'master_bookmark'" -i "None" -i "None"
  {"data": {"Err": {"code": 0,
                    "message": "invalid SetBookmarkRequest, must specify at least one of 'to' or 'from'"}}}

Test move bookmark failure (invalid from)
  $ hg debugapi -e setbookmark -i "'master_bookmark'" -i "'$D'" -i "'$C'"
  {"data": {"Err": {"code": 0,
                    "message": "invalid request: Bookmark transaction failed"}}}

Inspect results
  $ hg pull -q
  $ hg log -G -T '{node} {desc} {remotenames}\n' -r "all()"
  o  2576855b2ced4f17d5cf3daa80dd1b9d4b35ddce E remote/master_bookmark
  │
  o  74dbcd84493ad579ee26bb326c4272983098f69c D
  │
  o  d3b399ca8757acdb81c3681b052eb978db6768d8 C
  │
  o  80521a640a0c8f51dcc128c2658b224d595840ac B remote/create_bookmark
  │
  o  20ca2a4749a439b459125ef0f6a4f26e88ee7538 A
  



Test delete bookmark failure (invalid from)
  $ hg debugapi -e setbookmark -i "'create_bookmark'" -i "None" -i "'$D'"
  {"data": {"Err": {"code": 0,
                    "message": "invalid request: Bookmark transaction failed"}}}

Inspect results
  $ hg pull -q
  $ hg log -G -T '{node} {desc} {remotenames}\n' -r "all()"
  o  2576855b2ced4f17d5cf3daa80dd1b9d4b35ddce E remote/master_bookmark
  │
  o  74dbcd84493ad579ee26bb326c4272983098f69c D
  │
  o  d3b399ca8757acdb81c3681b052eb978db6768d8 C
  │
  o  80521a640a0c8f51dcc128c2658b224d595840ac B remote/create_bookmark
  │
  o  20ca2a4749a439b459125ef0f6a4f26e88ee7538 A
  



Test create bookmark failure (already exists)
  $ hg debugapi -e setbookmark -i "'create_bookmark'" -i "'$D'" -i "None"
  {"data": {"Err": {"code": 0,
                    "message": "invalid request: Bookmark transaction failed"}}}

Inspect results
  $ hg pull -q
  $ hg log -G -T '{node} {desc} {remotenames}\n' -r "all()"
  o  2576855b2ced4f17d5cf3daa80dd1b9d4b35ddce E remote/master_bookmark
  │
  o  74dbcd84493ad579ee26bb326c4272983098f69c D
  │
  o  d3b399ca8757acdb81c3681b052eb978db6768d8 C
  │
  o  80521a640a0c8f51dcc128c2658b224d595840ac B remote/create_bookmark
  │
  o  20ca2a4749a439b459125ef0f6a4f26e88ee7538 A
  
