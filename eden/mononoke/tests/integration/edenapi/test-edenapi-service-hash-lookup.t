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
  $ testtool_drawdag -R repo --print-hg-hashes <<EOF
  > D   F
  > |   |
  > C   E
  >  \ /
  >   B
  >   |
  >   A
  > # bookmark: D master_bookmark
  > EOF
  A=20ca2a4749a439b459125ef0f6a4f26e88ee7538
  B=80521a640a0c8f51dcc128c2658b224d595840ac
  C=d3b399ca8757acdb81c3681b052eb978db6768d8
  D=74dbcd84493ad579ee26bb326c4272983098f69c
  E=a66a30bed387971d9b4505eff1d9599dc16c141a
  F=d6e9a5359dcbb3b00616ebba901199b45d039851

Import and start mononoke
  $ cd ..
  $ setup_mononoke_config
  $ start_and_wait_for_mononoke_server

Check response - test hash lookup with actual commit hashes
First test: look up commits in range starting with 'd' (should find commits C and F)
  $ hg debugapi mono:repo -e hashlookup -i "[\"d\", \"$A\", \"\", \"ffff\"]"
  [{"hgids": [bin("d3b399ca8757acdb81c3681b052eb978db6768d8"),
              bin("d6e9a5359dcbb3b00616ebba901199b45d039851")],
    "request": {"InclusiveRange": [bin("d000000000000000000000000000000000000000"),
                                   bin("dfffffffffffffffffffffffffffffffffffffff")]}},
   {"hgids": [bin("20ca2a4749a439b459125ef0f6a4f26e88ee7538")],
    "request": {"InclusiveRange": [bin("20ca2a4749a439b459125ef0f6a4f26e88ee7538"),
                                   bin("20ca2a4749a439b459125ef0f6a4f26e88ee7538")]}},
   {"hgids": [bin("20ca2a4749a439b459125ef0f6a4f26e88ee7538"),
              bin("74dbcd84493ad579ee26bb326c4272983098f69c"),
              bin("80521a640a0c8f51dcc128c2658b224d595840ac"),
              bin("a66a30bed387971d9b4505eff1d9599dc16c141a"),
              bin("d3b399ca8757acdb81c3681b052eb978db6768d8"),
              bin("d6e9a5359dcbb3b00616ebba901199b45d039851")],
    "request": {"InclusiveRange": [bin("0000000000000000000000000000000000000000"),
                                   bin("ffffffffffffffffffffffffffffffffffffffff")]}},
   {"hgids": [],
    "request": {"InclusiveRange": [bin("ffff000000000000000000000000000000000000"),
                                   bin("ffffffffffffffffffffffffffffffffffffffff")]}}]
