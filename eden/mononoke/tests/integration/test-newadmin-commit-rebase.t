# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config

  $ testtool_drawdag -R repo << 'EOF'
  >   G
  >   |
  >   F
  >   |
  > C E
  > |/
  > B D
  > |/
  > A
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  D=fa8ba037ceed6e3f11f3bd0d21a866ca4c7a8c721ff13ca7c0b3442e1e4cbb16
  E=34a8be8c9dc6e46c954a330f06f792cb62a133d3f7ce9b46e70b91d358970c70
  F=7d7e98c04f7dd3f4be0916b4f0b95fa44747b065f105bd36b87252356ae0d0f5
  G=afd6205b79a8c12bece065ad3788632f1dca072ccb7d0802237bf10e7ad4a620

  $ mononoke_newadmin commit -R repo rebase -s $D -d $C --skip-rebase-validity-checks
  f41e886d61d03021b73d006acf237244086eb7a5d9c7989e44e59b76d3c3f2b5

  $ mononoke_newadmin changelog -R repo graph -i $G,f41e886d61d03021b73d006acf237244086eb7a5d9c7989e44e59b76d3c3f2b5 -M -I
  o  message: G, id: afd6205b79a8c12bece065ad3788632f1dca072ccb7d0802237bf10e7ad4a620
  │
  │ o  message: D, id: f41e886d61d03021b73d006acf237244086eb7a5d9c7989e44e59b76d3c3f2b5
  │ │
  o │  message: F, id: 7d7e98c04f7dd3f4be0916b4f0b95fa44747b065f105bd36b87252356ae0d0f5
  │ │
  │ o  message: C, id: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  │ │
  o │  message: E, id: 34a8be8c9dc6e46c954a330f06f792cb62a133d3f7ce9b46e70b91d358970c70
  ├─╯
  o  message: B, id: f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  │
  o  message: A, id: aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675

  $ mononoke_newadmin commit -R repo rebase -s $E -t $G -d $C --skip-rebase-validity-checks
  c4300546b70e88ad3c00bc1014c68a182782d089cadb54fec05b1d8790580c3a
  6ce9ac91c1bf4fe665c1817373ec92e0a02ee14476a054b624ed2d0f6e8db9a8
  b22799d5d4e4bfbce8b21b0a073739d999e51add1e087ff940fe65ba68c5e58b

  $ mononoke_newadmin changelog -R repo graph -i b22799d5d4e4bfbce8b21b0a073739d999e51add1e087ff940fe65ba68c5e58b,f41e886d61d03021b73d006acf237244086eb7a5d9c7989e44e59b76d3c3f2b5 -M -I
  o  message: G, id: b22799d5d4e4bfbce8b21b0a073739d999e51add1e087ff940fe65ba68c5e58b
  │
  o  message: F, id: 6ce9ac91c1bf4fe665c1817373ec92e0a02ee14476a054b624ed2d0f6e8db9a8
  │
  │ o  message: D, id: f41e886d61d03021b73d006acf237244086eb7a5d9c7989e44e59b76d3c3f2b5
  │ │
  o │  message: E, id: c4300546b70e88ad3c00bc1014c68a182782d089cadb54fec05b1d8790580c3a
  ├─╯
  o  message: C, id: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  │
  o  message: B, id: f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  │
  o  message: A, id: aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
