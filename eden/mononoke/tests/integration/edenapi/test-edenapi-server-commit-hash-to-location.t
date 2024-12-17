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
  > # bookmark: G master_bookmark
  > EOF
  A=20ca2a4749a439b459125ef0f6a4f26e88ee7538
  B=80521a640a0c8f51dcc128c2658b224d595840ac
  C=d3b399ca8757acdb81c3681b052eb978db6768d8
  D=74dbcd84493ad579ee26bb326c4272983098f69c
  E=2576855b2ced4f17d5cf3daa80dd1b9d4b35ddce
  F=f62bbd4a42386754afbb006e366387d8dc03687c
  G=ca68c421d5180ec8ca69aae4746952dd6be3e1c3



Start up SaplingRemoteAPI server.
  $ setup_mononoke_config
  $ start_and_wait_for_mononoke_server

Clone repo
  $ hg clone -q mono:repo repo
  $ cd repo

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
  [{"hgid": bin("f62bbd4a42386754afbb006e366387d8dc03687c"),
    "result": {"Ok": {"distance": 0,
                      "descendant": bin("f62bbd4a42386754afbb006e366387d8dc03687c")}}},
   {"hgid": bin("2576855b2ced4f17d5cf3daa80dd1b9d4b35ddce"),
    "result": {"Ok": {"distance": 1,
                      "descendant": bin("f62bbd4a42386754afbb006e366387d8dc03687c")}}},
   {"hgid": bin("74dbcd84493ad579ee26bb326c4272983098f69c"),
    "result": {"Ok": {"distance": 2,
                      "descendant": bin("f62bbd4a42386754afbb006e366387d8dc03687c")}}},
   {"hgid": bin("d3b399ca8757acdb81c3681b052eb978db6768d8"),
    "result": {"Ok": {"distance": 3,
                      "descendant": bin("f62bbd4a42386754afbb006e366387d8dc03687c")}}},
   {"hgid": bin("80521a640a0c8f51dcc128c2658b224d595840ac"),
    "result": {"Ok": {"distance": 4,
                      "descendant": bin("f62bbd4a42386754afbb006e366387d8dc03687c")}}},
   {"hgid": bin("20ca2a4749a439b459125ef0f6a4f26e88ee7538"),
    "result": {"Ok": {"distance": 5,
                      "descendant": bin("f62bbd4a42386754afbb006e366387d8dc03687c")}}},
   {"hgid": bin("f62bbd4a42386754afbb006e366387d8dc03687c"),
    "result": {"Ok": {"distance": 0,
                      "descendant": bin("f62bbd4a42386754afbb006e366387d8dc03687c")}}},
   {"hgid": bin("ca68c421d5180ec8ca69aae4746952dd6be3e1c3"),
    "result": {"Ok": None}},
   {"hgid": bin("000000000000000000000000000000123456789a"),
    "result": {"Ok": None}}]
