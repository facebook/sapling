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
  >   F
  >   |
  > D |
  > | E
  > C |
  >  \|
  >   B
  >   |
  >   A
  > EOS
  $ hg log -G -r "all()" -T "{desc} {node}\n"
  o  F 11abe3fb10b8689b560681094b17fe161871d043
  │
  │ o  D f585351a92f85104bff7c284233c338b10eb1df7
  │ │
  o │  E 49cb92066bfd0763fff729c354345650b7428554
  │ │
  │ o  C 26805aba1e600a82e93661149f2313866a221a7b
  ├─╯
  o  B 112478962961147124edd43549aedd1a335e44bf
  │
  o  A 426bada5c67598ca65036d57d9e4b64b0c1ce7a0
  

Import and start mononoke
  $ cd ..
  $ blobimport repo-hg/.hg repo
  $ SEGMENTED_CHANGELOG_ENABLE=1 setup_mononoke_config
  $ mononoke
  $ wait_for_mononoke

Check response.
  $ hgedenapi debugapi -e hashlookup -i '["4", "26805aba1e600a82e93661149f2313866a221a7b", "", "ffff"]'
  [{"hgids": [bin("426bada5c67598ca65036d57d9e4b64b0c1ce7a0"),
              bin("49cb92066bfd0763fff729c354345650b7428554")],
    "request": {"InclusiveRange": [bin("4000000000000000000000000000000000000000"),
                                   bin("4fffffffffffffffffffffffffffffffffffffff")]}},
   {"hgids": [bin("26805aba1e600a82e93661149f2313866a221a7b")],
    "request": {"InclusiveRange": [bin("26805aba1e600a82e93661149f2313866a221a7b"),
                                   bin("26805aba1e600a82e93661149f2313866a221a7b")]}},
   {"hgids": [bin("112478962961147124edd43549aedd1a335e44bf"),
              bin("11abe3fb10b8689b560681094b17fe161871d043"),
              bin("26805aba1e600a82e93661149f2313866a221a7b"),
              bin("426bada5c67598ca65036d57d9e4b64b0c1ce7a0"),
              bin("49cb92066bfd0763fff729c354345650b7428554"),
              bin("f585351a92f85104bff7c284233c338b10eb1df7")],
    "request": {"InclusiveRange": [bin("0000000000000000000000000000000000000000"),
                                   bin("ffffffffffffffffffffffffffffffffffffffff")]}},
   {"hgids": [],
    "request": {"InclusiveRange": [bin("ffff000000000000000000000000000000000000"),
                                   bin("ffffffffffffffffffffffffffffffffffffffff")]}}]
