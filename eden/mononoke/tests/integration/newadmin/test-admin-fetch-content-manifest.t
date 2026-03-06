# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.
#require slow

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ ADDITIONAL_DERIVED_DATA="content_manifests" setup_common_config "blob_sqlite"
  $ mononoke_testtool drawdag -R repo --derive-all <<'EOF'
  > A-B-C
  > # bookmark: C main
  > # extra: A example_extra "123\xff"
  > EOF
  A=c1c5eb4a15a4c71edae31c84f8b23ec5008ad16be07fba5b872fe010184b16ba
  B=749add4e33cf83fda6cce6f4fb4e3037a171dd8068acef09b336fd8ae027bf6f
  C=93cd0903625ea3162047e2699c2ea20d531b634df84180dbeeeb4b62f8afa8cd

Enable content manifests via JustKnobs
  $ merge_just_knobs <<EOF
  > {
  >   "bools": {
  >     "scm/mononoke:derived_data_use_content_manifests": true
  >   }
  > }
  > EOF

  $ mononoke_admin fetch -R repo -B main -p "" -k content-manifest
  Summary:
  Children: 3 files (3), 0 dirs
  Descendants: 3 files (3), 0 dirs
  Children list:
  A eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9 regular
  B 55662471e2a28db8257939b2f9a2d24e65b46a758bac12914a58f17dcde6905f regular
  C 896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d regular

  $ mononoke_admin fetch -R repo -B main -p "A" -k content-manifest
  File-Type: regular
  Size: 1
  Content-Id: eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9
  Sha1: 6dcd4ce23d88e2ee9568ba546c007c63d9131c1b
  Sha256: 559aead08264d5795d3909718cdd05abd49572e84fe55590eef31a88a08fdffd
  Git-Sha1: 8c7e5a667f1b771847fe88c01c3de34413a1b220
  
  A
