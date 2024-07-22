# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.
#require slow

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ ENABLED_DERIVED_DATA='["unodes"]' OTHER_DERIVED_DATA='["skeleton_manifests"]' setup_common_config

  $ testtool_drawdag -R repo << 'EOF'
  > D F
  > | |
  > C E
  > |/
  > B
  > |
  > A
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  D=f41e886d61d03021b73d006acf237244086eb7a5d9c7989e44e59b76d3c3f2b5
  E=34a8be8c9dc6e46c954a330f06f792cb62a133d3f7ce9b46e70b91d358970c70
  F=7d7e98c04f7dd3f4be0916b4f0b95fa44747b065f105bd36b87252356ae0d0f5


Deriving unodes using the "default" config succeeds since it's enabled
  $ mononoke_newadmin derived-data -R repo derive -T unodes -i $D
  $ mononoke_newadmin derived-data -R repo exists -T unodes -i $D
  Derived: f41e886d61d03021b73d006acf237244086eb7a5d9c7989e44e59b76d3c3f2b5

Deriving skeleton_manifests using the "default" config fails because it's not enabled
  $ mononoke_newadmin derived-data -R repo derive -T skeleton_manifests -i $D
  Error: Derivation of skeleton_manifests is not enabled for repo=repo repoid=0
  [1]
  $ mononoke_newadmin derived-data -R repo -c other exists -T skeleton_manifests -i $D
  Not Derived: f41e886d61d03021b73d006acf237244086eb7a5d9c7989e44e59b76d3c3f2b5

Deriving unodes using the "other" config fails
  $ mononoke_newadmin derived-data -R repo -c other derive -T unodes -i $F
  Error: Derivation of unodes is not enabled for repo=repo repoid=0
  [1]
  $ mononoke_newadmin derived-data -R repo exists -T unodes -i $F
  Not Derived: 7d7e98c04f7dd3f4be0916b4f0b95fa44747b065f105bd36b87252356ae0d0f5

Deriving skeleton_manifests using the "other" config succeeds
  $ mononoke_newadmin derived-data -R repo -c other derive -T skeleton_manifests -i $F
  $ mononoke_newadmin derived-data -R repo -c other exists -T skeleton_manifests -i $F
  Derived: 7d7e98c04f7dd3f4be0916b4f0b95fa44747b065f105bd36b87252356ae0d0f5
