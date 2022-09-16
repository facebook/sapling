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
  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ setup_hg_server

Populate test repo
  $ drawdag << EOS
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
  > EOS
  $ hg bookmark -r "$G" "master_bookmark"
  $ hg log -G -T '{node} {desc}\n' -r "all()"
  o  a22ebc2f5947b439a77147f07f4f3fe43355bfa3 H
  │
  │ o  9bc730a19041f9ec7cb33c626e811aa233efb18c E
  │ │
  o │  181938a6b0e46aedfaf17b5866659716bf974efa G
  │ │
  │ o  f585351a92f85104bff7c284233c338b10eb1df7 D
  │ │
  o │  33441538d4aad7db588e456862f21997f3bc7a04 F
  │ │
  │ o  26805aba1e600a82e93661149f2313866a221a7b C
  ├─╯
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

Test land stack
  $ hgedenapi debugapi -e landstack -i "'master_bookmark'" -i "'$E'" -i "'$B'"
  {"new_head": bin("cee85bb77dff9258b0b36fbe83501f3fd953fc4d"),
   "old_to_new_hgids": {bin("26805aba1e600a82e93661149f2313866a221a7b"): bin("fe5e845d9af57038d1cd62d4c10a61dd52655389"),
                        bin("9bc730a19041f9ec7cb33c626e811aa233efb18c"): bin("cee85bb77dff9258b0b36fbe83501f3fd953fc4d"),
                        bin("f585351a92f85104bff7c284233c338b10eb1df7"): bin("c5ef64ddf563718659b4c9777f0110de43055135")}}

Inspect results
  $ hgedenapi pull -q
  $ hgedenapi log -G -T '{node} {desc}\n' -r "sort(all(),topo)"
  o  cee85bb77dff9258b0b36fbe83501f3fd953fc4d E
  │
  o  c5ef64ddf563718659b4c9777f0110de43055135 D
  │
  o  fe5e845d9af57038d1cd62d4c10a61dd52655389 C
  │
  │ o  a22ebc2f5947b439a77147f07f4f3fe43355bfa3 H
  ├─╯
  o  181938a6b0e46aedfaf17b5866659716bf974efa G
  │
  o  33441538d4aad7db588e456862f21997f3bc7a04 F
  │
  │ o  9bc730a19041f9ec7cb33c626e811aa233efb18c E
  │ │
  │ o  f585351a92f85104bff7c284233c338b10eb1df7 D
  │ │
  │ o  26805aba1e600a82e93661149f2313866a221a7b C
  ├─╯
  o  112478962961147124edd43549aedd1a335e44bf B
  │
  o  426bada5c67598ca65036d57d9e4b64b0c1ce7a0 A
  
