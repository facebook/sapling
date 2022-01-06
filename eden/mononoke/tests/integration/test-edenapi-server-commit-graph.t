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
  >   G
  >   |
  >   F
  >  /|
  > D |
  > | E
  > C |
  >  \|
  >   B
  >   |
  >   A
  > EOS
  $ hg bookmark -r "$H" "master_bookmark"
  $ hg log -G -T '{node} {desc}\n' -r "all()"
  o  06383dd46c9bcbca9300252b4b6cddad88f8af21 H
  │
  o  1b794c59b583e47686701d0142848e90a3a94a7d G
  │
  o    bb56d4161ee371c720dbc8b504810c62a22fe314 F
  ├─╮
  │ o  f585351a92f85104bff7c284233c338b10eb1df7 D
  │ │
  o │  49cb92066bfd0763fff729c354345650b7428554 E
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
  $ SEGMENTED_CHANGELOG_ENABLE=1 setup_mononoke_config
  $ mononoke
  $ wait_for_mononoke

Check response.
  $ hgedenapi debugapi -e commitgraph -i "['$H']" -i "['$B','$C']" --sort
  [{"hgid": bin("49cb92066bfd0763fff729c354345650b7428554"),
    "parents": [bin("112478962961147124edd43549aedd1a335e44bf")]},
   {"hgid": bin("06383dd46c9bcbca9300252b4b6cddad88f8af21"),
    "parents": [bin("1b794c59b583e47686701d0142848e90a3a94a7d")]},
   {"hgid": bin("1b794c59b583e47686701d0142848e90a3a94a7d"),
    "parents": [bin("bb56d4161ee371c720dbc8b504810c62a22fe314")]},
   {"hgid": bin("bb56d4161ee371c720dbc8b504810c62a22fe314"),
    "parents": [bin("49cb92066bfd0763fff729c354345650b7428554"),
                bin("f585351a92f85104bff7c284233c338b10eb1df7")]},
   {"hgid": bin("f585351a92f85104bff7c284233c338b10eb1df7"),
    "parents": [bin("26805aba1e600a82e93661149f2313866a221a7b")]}]
