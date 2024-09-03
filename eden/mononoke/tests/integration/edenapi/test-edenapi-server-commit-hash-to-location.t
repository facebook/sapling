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

Populate test repo
  $ drawdag << EOS
  >   G
  >   |
  >   F
  >   |
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
  $ hg bookmark -r "$G" "master_bookmark"
  $ hg log -G -T '{node} {desc}\n' -r "all()"
  o  43195508e3bb704c08d24c40375bdd826789dd72 G
  │
  o  a194cadd16930608adaa649035ad4c16930cbd0f F
  │
  o  9bc730a19041f9ec7cb33c626e811aa233efb18c E
  │
  o  f585351a92f85104bff7c284233c338b10eb1df7 D
  │
  o  26805aba1e600a82e93661149f2313866a221a7b C
  │
  o  112478962961147124edd43549aedd1a335e44bf B
  │
  o  426bada5c67598ca65036d57d9e4b64b0c1ce7a0 A
  

Blobimport test repo.
  $ cd ..
  $ blobimport repo/.hg repo

Start up SaplingRemoteAPI server.
  $ setup_mononoke_config
  $ start_and_wait_for_mononoke_server
Create and send request.
  $ cat > master_heads << EOF
  > ["$F"]
  > EOF

  $ cat > hgids << EOF
  > [
  >     "$F",
  >     "$E",
  >     "$D",
  >     "$C",
  >     "$B",
  >     "$A",
  >     "$F",
  >     "$G",
  >     "000000000000000000000000000000123456789a"
  > ]
  > EOF

  $ hg debugapi mono:repo -e commithashtolocation -f master_heads -f hgids
  [{"hgid": bin("a194cadd16930608adaa649035ad4c16930cbd0f"),
    "result": {"Ok": {"distance": 0,
                      "descendant": bin("a194cadd16930608adaa649035ad4c16930cbd0f")}}},
   {"hgid": bin("9bc730a19041f9ec7cb33c626e811aa233efb18c"),
    "result": {"Ok": {"distance": 1,
                      "descendant": bin("a194cadd16930608adaa649035ad4c16930cbd0f")}}},
   {"hgid": bin("f585351a92f85104bff7c284233c338b10eb1df7"),
    "result": {"Ok": {"distance": 2,
                      "descendant": bin("a194cadd16930608adaa649035ad4c16930cbd0f")}}},
   {"hgid": bin("26805aba1e600a82e93661149f2313866a221a7b"),
    "result": {"Ok": {"distance": 3,
                      "descendant": bin("a194cadd16930608adaa649035ad4c16930cbd0f")}}},
   {"hgid": bin("112478962961147124edd43549aedd1a335e44bf"),
    "result": {"Ok": {"distance": 4,
                      "descendant": bin("a194cadd16930608adaa649035ad4c16930cbd0f")}}},
   {"hgid": bin("426bada5c67598ca65036d57d9e4b64b0c1ce7a0"),
    "result": {"Ok": {"distance": 5,
                      "descendant": bin("a194cadd16930608adaa649035ad4c16930cbd0f")}}},
   {"hgid": bin("a194cadd16930608adaa649035ad4c16930cbd0f"),
    "result": {"Ok": {"distance": 0,
                      "descendant": bin("a194cadd16930608adaa649035ad4c16930cbd0f")}}},
   {"hgid": bin("43195508e3bb704c08d24c40375bdd826789dd72"),
    "result": {"Ok": None}},
   {"hgid": bin("000000000000000000000000000000123456789a"),
    "result": {"Ok": None}}]
