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
  > # bookmark: H master_bookmark
  > EOF
  A=20ca2a4749a439b459125ef0f6a4f26e88ee7538
  B=80521a640a0c8f51dcc128c2658b224d595840ac
  C=d3b399ca8757acdb81c3681b052eb978db6768d8
  D=74dbcd84493ad579ee26bb326c4272983098f69c
  E=a66a30bed387971d9b4505eff1d9599dc16c141a
  F=15337eedcc780f51c80c21c3ed18d2d5ec0c28d9
  G=2685c553bdad5dc2f2ddc23ec329dad1eef1dc18
  H=7e61c4c6dfb99e9134f5a557d948e820e80e9e25




Start up SaplingRemoteAPI server.
  $ setup_mononoke_config
  $ start_and_wait_for_mononoke_server

Check response.
  $ hg debugapi mono:repo -e commitgraph -i "['$H']" -i "['$B','$C']" --sort
  [{"hgid": bin("2685c553bdad5dc2f2ddc23ec329dad1eef1dc18"),
    "parents": [bin("15337eedcc780f51c80c21c3ed18d2d5ec0c28d9")],
    "is_draft": True},
   {"hgid": bin("15337eedcc780f51c80c21c3ed18d2d5ec0c28d9"),
    "parents": [bin("74dbcd84493ad579ee26bb326c4272983098f69c"),
                bin("a66a30bed387971d9b4505eff1d9599dc16c141a")],
    "is_draft": True},
   {"hgid": bin("a66a30bed387971d9b4505eff1d9599dc16c141a"),
    "parents": [bin("80521a640a0c8f51dcc128c2658b224d595840ac")],
    "is_draft": True},
   {"hgid": bin("74dbcd84493ad579ee26bb326c4272983098f69c"),
    "parents": [bin("d3b399ca8757acdb81c3681b052eb978db6768d8")],
    "is_draft": True},
   {"hgid": bin("7e61c4c6dfb99e9134f5a557d948e820e80e9e25"),
    "parents": [bin("2685c553bdad5dc2f2ddc23ec329dad1eef1dc18")],
    "is_draft": True}]
