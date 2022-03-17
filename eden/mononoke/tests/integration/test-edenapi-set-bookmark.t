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
  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ setup_hg_server

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
  $ blobimport repo-hg/.hg repo

Start up EdenAPI server.
  $ ENABLE_API_WRITES=1 setup_mononoke_config
  $ start_and_wait_for_mononoke_server
Clone the repo
  $ cd $TESTTMP
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo2 --noupdate -q
  $ cd repo2
  $ setup_hg_client

Test move bookmark
  $ hgedenapi debugapi -e setbookmark -i "'master_bookmark'" -i "'$E'" -i "'$C'"
  True

Inspect results
  $ hgedenapi pull -q
  $ hgedenapi log -G -T '{node} {desc} {remotenames}\n' -r "all()"
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
  $ hgedenapi debugapi -e setbookmark -i "'to_delete'" -i "None" -i "'$E'"
  True

Inspect results
  $ hgedenapi pull -q
  $ hgedenapi log -G -T '{node} {desc} {remotenames}\n' -r "all()"
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
  $ hgedenapi debugapi -e setbookmark -i "'create_bookmark'" -i "'$B'" -i "None"
  True

Inspect results
  $ hgedenapi pull -q
  $ hgedenapi log -G -T '{node} {desc} {remotenames}\n' -r "all()"
  o  9bc730a19041f9ec7cb33c626e811aa233efb18c E default/master_bookmark
  │
  o  f585351a92f85104bff7c284233c338b10eb1df7 D
  │
  o  26805aba1e600a82e93661149f2313866a221a7b C
  │
  o  112478962961147124edd43549aedd1a335e44bf B default/create_bookmark
  │
  o  426bada5c67598ca65036d57d9e4b64b0c1ce7a0 A
  

Test move bookmark failure (invalid from)
  $ hgedenapi debugapi -e setbookmark -i "'master_bookmark'" -i "'$D'" -i "'$C'" 2>&1 | grep 'error.HttpError'
  error.HttpError: expected response, but none returned by the server

Inspect results
  $ hgedenapi pull -q
  $ hgedenapi log -G -T '{node} {desc} {remotenames}\n' -r "all()"
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
  $ hgedenapi debugapi -e setbookmark -i "'create_bookmark'" -i "None" -i "'$D'" 2>&1 | grep 'error.HttpError'
  error.HttpError: expected response, but none returned by the server

Inspect results
  $ hgedenapi pull -q
  $ hgedenapi log -G -T '{node} {desc} {remotenames}\n' -r "all()"
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
  $ hgedenapi debugapi -e setbookmark -i "'create_bookmark'" -i "'$D'" -i "None" 2>&1 | grep 'error.HttpError'
  error.HttpError: expected response, but none returned by the server

Inspect results
  $ hgedenapi pull -q
  $ hgedenapi log -G -T '{node} {desc} {remotenames}\n' -r "all()"
  o  9bc730a19041f9ec7cb33c626e811aa233efb18c E default/master_bookmark
  │
  o  f585351a92f85104bff7c284233c338b10eb1df7 D
  │
  o  26805aba1e600a82e93661149f2313866a221a7b C
  │
  o  112478962961147124edd43549aedd1a335e44bf B default/create_bookmark
  │
  o  426bada5c67598ca65036d57d9e4b64b0c1ce7a0 A
  
