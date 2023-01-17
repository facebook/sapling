# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Set up local hgrc and Mononoke config.
  $ quiet default_setup_blobimport
  $ setup_configerator_configs

Build up segmented changelog
  $ quiet segmented_changelog_tailer_reseed --repo repo --head=master_bookmark

Enable Segmented Changelog
  $ cat >> "$TESTTMP/mononoke-config/repos/repo/server.toml" <<CONFIG
  > [segmented_changelog_config]
  > enabled=true
  > CONFIG

  $ start_and_wait_for_mononoke_server
Test clone and other pull related endpoints

  $ hgedenapi debugapi -e clonedata
  {"idmap": {2: bin("26805aba1e600a82e93661149f2313866a221a7b")},
   "flat_segments": {"segments": [{"low": 0,
                                   "high": 2,
                                   "parents": []}]}}

  $ hgedenapi debugapi -e commitgraph -i '["26805aba1e600a82e93661149f2313866a221a7b"]' -i '[]' --sort
  [{"hgid": bin("26805aba1e600a82e93661149f2313866a221a7b"),
    "parents": [bin("112478962961147124edd43549aedd1a335e44bf")],
    "is_draft": False},
   {"hgid": bin("426bada5c67598ca65036d57d9e4b64b0c1ce7a0"),
    "parents": [],
    "is_draft": False},
   {"hgid": bin("112478962961147124edd43549aedd1a335e44bf"),
    "parents": [bin("426bada5c67598ca65036d57d9e4b64b0c1ce7a0")],
    "is_draft": False}]
  $ hgedenapi debugapi -e commitgraph -i '["26805aba1e600a82e93661149f2313866a221a7b"]' -i '["426bada5c67598ca65036d57d9e4b64b0c1ce7a0"]' --sort
  [{"hgid": bin("26805aba1e600a82e93661149f2313866a221a7b"),
    "parents": [bin("112478962961147124edd43549aedd1a335e44bf")],
    "is_draft": False},
   {"hgid": bin("112478962961147124edd43549aedd1a335e44bf"),
    "parents": [bin("426bada5c67598ca65036d57d9e4b64b0c1ce7a0")],
    "is_draft": False}]

  $ hgedenapi debugapi -e pulllazy -i '[]' -i '["26805aba1e600a82e93661149f2313866a221a7b"]'
  {"idmap": {0: bin("426bada5c67598ca65036d57d9e4b64b0c1ce7a0"),
             2: bin("26805aba1e600a82e93661149f2313866a221a7b")},
   "flat_segments": {"segments": [{"low": 0,
                                   "high": 2,
                                   "parents": []}]}}
  $ hgedenapi debugapi -e pulllazy -i '["426bada5c67598ca65036d57d9e4b64b0c1ce7a0"]' -i '["26805aba1e600a82e93661149f2313866a221a7b"]'
  {"idmap": {0: bin("426bada5c67598ca65036d57d9e4b64b0c1ce7a0"),
             1: bin("112478962961147124edd43549aedd1a335e44bf"),
             2: bin("26805aba1e600a82e93661149f2313866a221a7b")},
   "flat_segments": {"segments": [{"low": 1,
                                   "high": 2,
                                   "parents": [0]}]}}
