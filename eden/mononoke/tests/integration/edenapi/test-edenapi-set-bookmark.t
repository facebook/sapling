# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Set up local hgrc and Mononoke config.
  $ setup_common_config
  $ setup_configerator_configs
  $ enable remotenames
  $ cd $TESTTMP

Initialize test repo.
  $ hginit_treemanifest repo
  $ cd repo

Populate test repo
  $ drawdag << EOS
  >   E
  >   |
  >   D
  >   |
  >   C
  >   |
  >   B
  >   |
  >   A
  > EOS
  $ hg bookmark -r "$C" "master_bookmark"
  $ hg bookmark -r "$E" "to_delete"
  $ hg log -G -T '{node} {desc} {bookmarks}\n' -r "all()"
  o  9bc730a19041f9ec7cb33c626e811aa233efb18c E to_delete
  │
  o  f585351a92f85104bff7c284233c338b10eb1df7 D
  │
  o  26805aba1e600a82e93661149f2313866a221a7b C master_bookmark
  │
  o  112478962961147124edd43549aedd1a335e44bf B
  │
  o  426bada5c67598ca65036d57d9e4b64b0c1ce7a0 A
  

Blobimport test repo.
  $ cd ..
  $ blobimport repo/.hg repo

Start up SaplingRemoteAPI server.
  $ ENABLE_API_WRITES=1 setup_mononoke_config
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
  o  9bc730a19041f9ec7cb33c626e811aa233efb18c E default/master_bookmark default/to_delete
  │
  o  f585351a92f85104bff7c284233c338b10eb1df7 D
  │
  o  26805aba1e600a82e93661149f2313866a221a7b C
  │
  o  112478962961147124edd43549aedd1a335e44bf B
  │
  o  426bada5c67598ca65036d57d9e4b64b0c1ce7a0 A
  

Test delete bookmark
  $ hg debugapi -e setbookmark -i "'to_delete'" -i "None" -i "'$E'"
  {"data": {"Ok": None}}

Inspect results
  $ hg pull -q
  $ hg log -G -T '{node} {desc} {remotenames}\n' -r "all()"
  o  9bc730a19041f9ec7cb33c626e811aa233efb18c E default/master_bookmark
  │
  o  f585351a92f85104bff7c284233c338b10eb1df7 D
  │
  o  26805aba1e600a82e93661149f2313866a221a7b C
  │
  o  112478962961147124edd43549aedd1a335e44bf B
  │
  o  426bada5c67598ca65036d57d9e4b64b0c1ce7a0 A
  

Test create bookmark
  $ hg debugapi -e setbookmark -i "'create_bookmark'" -i "'$B'" -i "None"
  {"data": {"Ok": None}}

Inspect results
  $ hg pull -q
  $ hg log -G -T '{node} {desc} {remotenames}\n' -r "all()"
  o  9bc730a19041f9ec7cb33c626e811aa233efb18c E default/master_bookmark
  │
  o  f585351a92f85104bff7c284233c338b10eb1df7 D
  │
  o  26805aba1e600a82e93661149f2313866a221a7b C
  │
  o  112478962961147124edd43549aedd1a335e44bf B default/create_bookmark
  │
  o  426bada5c67598ca65036d57d9e4b64b0c1ce7a0 A
  

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
  o  9bc730a19041f9ec7cb33c626e811aa233efb18c E default/master_bookmark
  │
  o  f585351a92f85104bff7c284233c338b10eb1df7 D
  │
  o  26805aba1e600a82e93661149f2313866a221a7b C
  │
  o  112478962961147124edd43549aedd1a335e44bf B default/create_bookmark
  │
  o  426bada5c67598ca65036d57d9e4b64b0c1ce7a0 A
  


Test delete bookmark failure (invalid from)
  $ hg debugapi -e setbookmark -i "'create_bookmark'" -i "None" -i "'$D'"
  {"data": {"Err": {"code": 0,
                    "message": "invalid request: Bookmark transaction failed"}}}

Inspect results
  $ hg pull -q
  $ hg log -G -T '{node} {desc} {remotenames}\n' -r "all()"
  o  9bc730a19041f9ec7cb33c626e811aa233efb18c E default/master_bookmark
  │
  o  f585351a92f85104bff7c284233c338b10eb1df7 D
  │
  o  26805aba1e600a82e93661149f2313866a221a7b C
  │
  o  112478962961147124edd43549aedd1a335e44bf B default/create_bookmark
  │
  o  426bada5c67598ca65036d57d9e4b64b0c1ce7a0 A
  


Test create bookmark failure (already exists)
  $ hg debugapi -e setbookmark -i "'create_bookmark'" -i "'$D'" -i "None"
  {"data": {"Err": {"code": 0,
                    "message": "invalid request: Bookmark transaction failed"}}}

Inspect results
  $ hg pull -q
  $ hg log -G -T '{node} {desc} {remotenames}\n' -r "all()"
  o  9bc730a19041f9ec7cb33c626e811aa233efb18c E default/master_bookmark
  │
  o  f585351a92f85104bff7c284233c338b10eb1df7 D
  │
  o  26805aba1e600a82e93661149f2313866a221a7b C
  │
  o  112478962961147124edd43549aedd1a335e44bf B default/create_bookmark
  │
  o  426bada5c67598ca65036d57d9e4b64b0c1ce7a0 A
  
