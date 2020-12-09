# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Set up local hgrc and Mononoke config.
  $ SEGMENTED_CHANGELOG_ALWAYS_DOWNLOAD_SAVE=1 quiet default_setup_blobimport
  $ setup_configerator_configs

Build up segmented changelog
  $ quiet segmented_changelog_seeder --head=master_bookmark

Start up EdenAPI server.
  $ start_edenapi_server

  $ sslcurl -s "$EDENAPI_URI/repo/full_idmap_clone" -X POST > res.cbor

Check files in response.
  $ edenapi_read_res full-idmap-clone res.cbor
  Reading from file: "res.cbor"
  head_id: 2
  flat_segments: [
    0, 2, []
  ]
  idmap: {
    0: 426bada5c67598ca65036d57d9e4b64b0c1ce7a0
    1: 112478962961147124edd43549aedd1a335e44bf
    2: 26805aba1e600a82e93661149f2313866a221a7b
  }

