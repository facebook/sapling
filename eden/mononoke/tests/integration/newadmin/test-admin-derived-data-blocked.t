# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.


  $ . "${TEST_FIXTURES}/library.sh"

  $ setup_common_config
  $ testtool_drawdag -R repo <<'EOF'
  > A-B-C
  > # bookmark: C main
  > # bookmark: B stable
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2

  $ cat >> "$TESTTMP/mononoke-config/repos/repo/server.toml" << CONFIG
  > [derived_data_config.blocked_derivation.changesets]
  > "f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658" = { blocked_derived_data_types = ["unodes"] }
  > "e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2" = {}
  > CONFIG

  $ mononoke_admin derived-data -R repo derive -T skeleton_manifests -B main
  Error: Derivation of skeleton_manifests for e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2 is blocked in config
  [1]
  $ mononoke_admin derived-data -R repo derive -T skeleton_manifests -B stable
  $ mononoke_admin derived-data -R repo derive -T unodes -B main
  Error: Derivation of unodes for f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658 is blocked in config
  [1]
  $ mononoke_admin derived-data -R repo derive -T unodes -B stable
  Error: Derivation of unodes for f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658 is blocked in config
  [1]
  $ mononoke_admin derived-data -R repo derive -T blame -B stable
  Error: failed to derive dependent types
  
  Caused by:
      Derivation of unodes for f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658 is blocked in config
  [1]
