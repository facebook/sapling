# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.
#require slow

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config
  $ mononoke_testtool drawdag -R repo <<'EOF'
  >      G
  >      |
  >      F
  >     / \
  >    D   E     L
  >     \ /      |
  >      C       K
  >      |  H    |
  >      B /     J
  >      |/      |
  >      A       I
  > # bookmark: G first_bookmark
  > # bookmark: L second_bookmark
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  D=f41e886d61d03021b73d006acf237244086eb7a5d9c7989e44e59b76d3c3f2b5
  E=c4300546b70e88ad3c00bc1014c68a182782d089cadb54fec05b1d8790580c3a
  F=2c1fe61358291c356cf3367b66152fca1f3ee8e9e4a5129f59643100f91408f7
  G=417bd860e802f264f4b5cbdcf0cf14baf9c8ae6d6a1beedafbf02d44c7e4063d
  H=9cff72783886d8e1c03fc9420fe944b30e30f0518c2fbb65ea04bca9b7f880c0
  I=19fff6751f630d5c837e00b7b28260825b53e4160aa5c8b3f5067e9f1eb376a4
  J=48e9ea18a97b9d86bcb0f209d76313b2597c05fecf2cb0ba16f595f4d24c4355
  K=a461eb57dda3f53be7fc7056af3637eb235280191fe002c4f6675201508011ff
  L=e344c3733985ad4321993aba435251774ed2107b2f2f250a1acd32db374ec5c3

slice all ancestors of all bookmarks
  $ mononoke_admin derived-data -R repo slice -T test_sharded_manifests --slice-size 3 --all-bookmarks -o combined.json
  $ cat combined.json | jq '.[0]' > slices.json
  $ cat combined.json | jq '.[1]' > boundaries.json
  $ cat combined.json | jq .
  [
    [
      {
        "segments": [
          {
            "head": "a461eb57dda3f53be7fc7056af3637eb235280191fe002c4f6675201508011ff",
            "base": "19fff6751f630d5c837e00b7b28260825b53e4160aa5c8b3f5067e9f1eb376a4"
          }
        ]
      },
      {
        "segments": [
          {
            "head": "e344c3733985ad4321993aba435251774ed2107b2f2f250a1acd32db374ec5c3",
            "base": "e344c3733985ad4321993aba435251774ed2107b2f2f250a1acd32db374ec5c3"
          },
          {
            "head": "f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658",
            "base": "aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675"
          }
        ]
      },
      {
        "segments": [
          {
            "head": "c4300546b70e88ad3c00bc1014c68a182782d089cadb54fec05b1d8790580c3a",
            "base": "e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2"
          },
          {
            "head": "f41e886d61d03021b73d006acf237244086eb7a5d9c7989e44e59b76d3c3f2b5",
            "base": "f41e886d61d03021b73d006acf237244086eb7a5d9c7989e44e59b76d3c3f2b5"
          }
        ]
      },
      {
        "segments": [
          {
            "head": "417bd860e802f264f4b5cbdcf0cf14baf9c8ae6d6a1beedafbf02d44c7e4063d",
            "base": "2c1fe61358291c356cf3367b66152fca1f3ee8e9e4a5129f59643100f91408f7"
          }
        ]
      }
    ],
    [
      "a461eb57dda3f53be7fc7056af3637eb235280191fe002c4f6675201508011ff",
      "c4300546b70e88ad3c00bc1014c68a182782d089cadb54fec05b1d8790580c3a",
      "f41e886d61d03021b73d006acf237244086eb7a5d9c7989e44e59b76d3c3f2b5",
      "f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658"
    ]
  ]

derive slice heads (B, D, E, and K)
  $ mononoke_admin derived-data -R repo derive-slice -T test_sharded_manifests -f boundaries.json --mode boundaries

check that B, D, E and K were derived
  $ mononoke_admin derived-data -R repo exists -T test_sharded_manifests -i f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  Derived: f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  $ mononoke_admin derived-data -R repo exists -T test_sharded_manifests -i f41e886d61d03021b73d006acf237244086eb7a5d9c7989e44e59b76d3c3f2b5
  Derived: f41e886d61d03021b73d006acf237244086eb7a5d9c7989e44e59b76d3c3f2b5
  $ mononoke_admin derived-data -R repo exists -T test_sharded_manifests -i c4300546b70e88ad3c00bc1014c68a182782d089cadb54fec05b1d8790580c3a
  Derived: c4300546b70e88ad3c00bc1014c68a182782d089cadb54fec05b1d8790580c3a
  $ mononoke_admin derived-data -R repo exists -T test_sharded_manifests -i a461eb57dda3f53be7fc7056af3637eb235280191fe002c4f6675201508011ff
  Derived: a461eb57dda3f53be7fc7056af3637eb235280191fe002c4f6675201508011ff

check that A, C, F, G, H, I and J, L weren't derived
  $ mononoke_admin derived-data -R repo exists -T test_sharded_manifests -i aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  Not Derived: aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  $ mononoke_admin derived-data -R repo exists -T test_sharded_manifests -i e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  Not Derived: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ mononoke_admin derived-data -R repo exists -T test_sharded_manifests -i 2c1fe61358291c356cf3367b66152fca1f3ee8e9e4a5129f59643100f91408f7
  Not Derived: 2c1fe61358291c356cf3367b66152fca1f3ee8e9e4a5129f59643100f91408f7
  $ mononoke_admin derived-data -R repo exists -T test_sharded_manifests -i 417bd860e802f264f4b5cbdcf0cf14baf9c8ae6d6a1beedafbf02d44c7e4063d
  Not Derived: 417bd860e802f264f4b5cbdcf0cf14baf9c8ae6d6a1beedafbf02d44c7e4063d
  $ mononoke_admin derived-data -R repo exists -T test_sharded_manifests -i 9cff72783886d8e1c03fc9420fe944b30e30f0518c2fbb65ea04bca9b7f880c0
  Not Derived: 9cff72783886d8e1c03fc9420fe944b30e30f0518c2fbb65ea04bca9b7f880c0
  $ mononoke_admin derived-data -R repo exists -T test_sharded_manifests -i 19fff6751f630d5c837e00b7b28260825b53e4160aa5c8b3f5067e9f1eb376a4
  Not Derived: 19fff6751f630d5c837e00b7b28260825b53e4160aa5c8b3f5067e9f1eb376a4
  $ mononoke_admin derived-data -R repo exists -T test_sharded_manifests -i 48e9ea18a97b9d86bcb0f209d76313b2597c05fecf2cb0ba16f595f4d24c4355
  Not Derived: 48e9ea18a97b9d86bcb0f209d76313b2597c05fecf2cb0ba16f595f4d24c4355
  $ mononoke_admin derived-data -R repo exists -T test_sharded_manifests -i e344c3733985ad4321993aba435251774ed2107b2f2f250a1acd32db374ec5c3
  Not Derived: e344c3733985ad4321993aba435251774ed2107b2f2f250a1acd32db374ec5c3

derive the rest of the slices (A, C, F, G, H, I, J, K and L)
  $ mononoke_admin derived-data -R repo derive-slice -T test_sharded_manifests -f slices.json --mode slices

check that A, C, F, G, H, I and J, K, L are now derived
  $ mononoke_admin derived-data -R repo exists -T test_sharded_manifests -i aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  Derived: aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  $ mononoke_admin derived-data -R repo exists -T test_sharded_manifests -i e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  Derived: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ mononoke_admin derived-data -R repo exists -T test_sharded_manifests -i 2c1fe61358291c356cf3367b66152fca1f3ee8e9e4a5129f59643100f91408f7
  Derived: 2c1fe61358291c356cf3367b66152fca1f3ee8e9e4a5129f59643100f91408f7
  $ mononoke_admin derived-data -R repo exists -T test_sharded_manifests -i 417bd860e802f264f4b5cbdcf0cf14baf9c8ae6d6a1beedafbf02d44c7e4063d
  Derived: 417bd860e802f264f4b5cbdcf0cf14baf9c8ae6d6a1beedafbf02d44c7e4063d
  $ mononoke_admin derived-data -R repo exists -T test_sharded_manifests -i 19fff6751f630d5c837e00b7b28260825b53e4160aa5c8b3f5067e9f1eb376a4
  Derived: 19fff6751f630d5c837e00b7b28260825b53e4160aa5c8b3f5067e9f1eb376a4
  $ mononoke_admin derived-data -R repo exists -T test_sharded_manifests -i 48e9ea18a97b9d86bcb0f209d76313b2597c05fecf2cb0ba16f595f4d24c4355
  Derived: 48e9ea18a97b9d86bcb0f209d76313b2597c05fecf2cb0ba16f595f4d24c4355
  $ mononoke_admin derived-data -R repo exists -T test_sharded_manifests -i a461eb57dda3f53be7fc7056af3637eb235280191fe002c4f6675201508011ff
  Derived: a461eb57dda3f53be7fc7056af3637eb235280191fe002c4f6675201508011ff
  $ mononoke_admin derived-data -R repo exists -T test_sharded_manifests -i e344c3733985ad4321993aba435251774ed2107b2f2f250a1acd32db374ec5c3
  Derived: e344c3733985ad4321993aba435251774ed2107b2f2f250a1acd32db374ec5c3

check that H is still not derived as it's not an ancestor of any bookmark
  $ mononoke_admin derived-data -R repo exists -T test_sharded_manifests -i 9cff72783886d8e1c03fc9420fe944b30e30f0518c2fbb65ea04bca9b7f880c0
  Not Derived: 9cff72783886d8e1c03fc9420fe944b30e30f0518c2fbb65ea04bca9b7f880c0

test --keep-going flag for derive-slice in boundaries mode
create a boundaries file with valid changesets (B, D) and one fake changeset.
use test_sharded_manifests with --rederive since B and D are already derived from the earlier test.
  $ python3 -c "import json; print(json.dumps(['f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658', '0000000000000000000000000000000000000000000000000000000000001234', 'f41e886d61d03021b73d006acf237244086eb7a5d9c7989e44e59b76d3c3f2b5']))" > keep_going_boundaries.json

derive-slice boundaries with --keep-going should rederive valid boundaries despite fake one failing
  $ mononoke_admin derived-data -R repo derive-slice -T test_sharded_manifests -f keep_going_boundaries.json --mode boundaries --keep-going --rederive 2>"$TESTTMP/keep_going_boundaries_err.txt"
  [1]
  $ grep "boundaries failed to derive" "$TESTTMP/keep_going_boundaries_err.txt"
  Error: 1 out of 3 boundaries failed to derive

verify valid boundaries (B, D) are still derived
  $ mononoke_admin derived-data -R repo exists -T test_sharded_manifests -i f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  Derived: f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  $ mononoke_admin derived-data -R repo exists -T test_sharded_manifests -i f41e886d61d03021b73d006acf237244086eb7a5d9c7989e44e59b76d3c3f2b5
  Derived: f41e886d61d03021b73d006acf237244086eb7a5d9c7989e44e59b76d3c3f2b5

test --keep-going flag for derive-slice in slices mode
create a slices file with one valid slice (changeset A) and one with a fake changeset
  $ python3 -c "
  > import json
  > slices = [
  >     {'segments': [{'head': 'aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675', 'base': 'aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675'}]},
  >     {'segments': [{'head': '0000000000000000000000000000000000000000000000000000000000001234', 'base': '0000000000000000000000000000000000000000000000000000000000001234'}]}
  > ]
  > print(json.dumps(slices))
  > " > keep_going_slices.json

derive-slice slices with --keep-going should derive valid slices despite fake one failing
  $ mononoke_admin derived-data -R repo derive-slice -T test_sharded_manifests -f keep_going_slices.json --mode slices --keep-going 2>"$TESTTMP/keep_going_slices_err.txt"
  [1]
  $ grep "slices failed to derive" "$TESTTMP/keep_going_slices_err.txt"
  Error: 1 out of 2 slices failed to derive

verify valid slice changeset (A) is still derived
  $ mononoke_admin derived-data -R repo exists -T test_sharded_manifests -i aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  Derived: aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
