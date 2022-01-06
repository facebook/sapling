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
  $ quiet segmented_changelog_seeder --head=master_bookmark

Enable Segmented Changelog
  $ cat >> "$TESTTMP/mononoke-config/repos/repo/server.toml" <<CONFIG
  > [segmented_changelog_config]
  > enabled=true
  > CONFIG

  $ mononoke
  $ wait_for_mononoke

Check response.
  $ hgedenapi debugapi -e clonedata
  {"idmap": {2: bin("26805aba1e600a82e93661149f2313866a221a7b")},
   "flat_segments": {"segments": [{"low": 0,
                                   "high": 2,
                                   "parents": []}]}}
