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
  $ hg bookmark -r "$F" "master_bookmark"
  $ log -r "all()"
  o  F [draft;rev=5;11abe3fb10b8]
  │
  │ o  D [draft;rev=4;f585351a92f8]
  │ │
  o │  E [draft;rev=3;49cb92066bfd]
  │ │
  │ o  C [draft;rev=2;26805aba1e60]
  ├─╯
  o  B [draft;rev=1;112478962961]
  │
  o  A [draft;rev=0;426bada5c675]
  $

Import and start mononoke
  $ cd ..
  $ blobimport repo-hg/.hg repo
  $ SEGMENTED_CHANGELOG_ENABLE=1 setup_mononoke_config
  $ mononoke
  $ wait_for_mononoke

Prepare request.
  $ cat > master_heads << EOF
  > [
  >     "$D",
  >     "$E"
  > ]
  > EOF

  $ cat > hgids << EOF
  > [
  >     "$B",
  >     "$C"
  > ]
  > EOF

Check response.
  $ hgedenapi debugapi -e commithashtolocation -f master_heads -f hgids
  [{"hgid": bin("112478962961147124edd43549aedd1a335e44bf"),
    "result": {"Err": {"code": 1,
                       "message": "InternalError(InternalError(error while getting an up to date dag\n\nCaused by:\n    server cannot match the clients heads, repo 0, client_heads: [ChangesetId(Blake2(86de925f9338cbc325f5ec1620b6556fb441d1e08466f65ae51930fae6abe120)), ChangesetId(Blake2(582452dd18b423e3212130c383fcf9b31ee52e215a5df6f45fc594b3d48df3e4))]))"}}},
   {"hgid": bin("26805aba1e600a82e93661149f2313866a221a7b"),
    "result": {"Err": {"code": 1,
                       "message": "InternalError(InternalError(error while getting an up to date dag\n\nCaused by:\n    server cannot match the clients heads, repo 0, client_heads: [ChangesetId(Blake2(86de925f9338cbc325f5ec1620b6556fb441d1e08466f65ae51930fae6abe120)), ChangesetId(Blake2(582452dd18b423e3212130c383fcf9b31ee52e215a5df6f45fc594b3d48df3e4))]))"}}}]
