# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config

  $ testtool_drawdag -R repo << 'EOF'
  > J G L
  > | | |
  > I F K
  > | |/
  > H E
  > |/|
  > C D
  > |/
  > B
  > |
  > A
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  D=5a25c0a76794bbcc5180da0949a652750101597f0fbade488e611d5c0917e7be
  E=f0c81a03319da010415f712831abe8469ba3c30b93b0b07af175302b8c15f0e6
  F=48779d8d497815015031dc3f3e9888abc8cf8273184ebd9ca8a395e24d501c90
  G=9711852ec4f4b42937dd5b760c7b3f84345bf48c74b7ef3ca7118d1d7928744d
  H=64642fc5a09343c1699e2ecaa5fa1c31fdb19f1e125428cd745327911c0b1d83
  I=03ffabc887d3d9a81be514037b1dfa3020466af9145bafbc33a8880fd8808c01
  J=55e5dbaa7f26e0cfa1c2ee95479e2af088bf81caae4c2356d6eb8dfa6c114284
  K=446db3567bd27d9e2d8a7653a7db9d75cdbedf57d55e654410545574c2688b62
  L=756352f77488f7b1f4ef04c2e90fdfb0acec41a1db4118e023af8dc83fd0c344

returns nothing as there are no ancestors of C that are not also ancestors of I
  $ mononoke_newadmin commit-graph -R repo segments --heads $C --common $I --verify

returns a segment representing E, F and G and a segment representing D
  $ mononoke_newadmin commit-graph -R repo segments --heads $G --common $H --verify
  9711852ec4f4b42937dd5b760c7b3f84345bf48c74b7ef3ca7118d1d7928744d -> f0c81a03319da010415f712831abe8469ba3c30b93b0b07af175302b8c15f0e6, length: 3, parents: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2, 5a25c0a76794bbcc5180da0949a652750101597f0fbade488e611d5c0917e7be (5a25c0a76794bbcc5180da0949a652750101597f0fbade488e611d5c0917e7be~0)
  5a25c0a76794bbcc5180da0949a652750101597f0fbade488e611d5c0917e7be -> 5a25c0a76794bbcc5180da0949a652750101597f0fbade488e611d5c0917e7be, length: 1, parents: f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658

returns 4 segments representing G-F, L-E, J-H, D
  $ mononoke_newadmin commit-graph -R repo segments --heads $J,$G,$L --common $C --verify
  9711852ec4f4b42937dd5b760c7b3f84345bf48c74b7ef3ca7118d1d7928744d -> 48779d8d497815015031dc3f3e9888abc8cf8273184ebd9ca8a395e24d501c90, length: 2, parents: f0c81a03319da010415f712831abe8469ba3c30b93b0b07af175302b8c15f0e6 (756352f77488f7b1f4ef04c2e90fdfb0acec41a1db4118e023af8dc83fd0c344~2)
  756352f77488f7b1f4ef04c2e90fdfb0acec41a1db4118e023af8dc83fd0c344 -> f0c81a03319da010415f712831abe8469ba3c30b93b0b07af175302b8c15f0e6, length: 3, parents: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2, 5a25c0a76794bbcc5180da0949a652750101597f0fbade488e611d5c0917e7be (5a25c0a76794bbcc5180da0949a652750101597f0fbade488e611d5c0917e7be~0)
  55e5dbaa7f26e0cfa1c2ee95479e2af088bf81caae4c2356d6eb8dfa6c114284 -> 64642fc5a09343c1699e2ecaa5fa1c31fdb19f1e125428cd745327911c0b1d83, length: 3, parents: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  5a25c0a76794bbcc5180da0949a652750101597f0fbade488e611d5c0917e7be -> 5a25c0a76794bbcc5180da0949a652750101597f0fbade488e611d5c0917e7be, length: 1, parents: f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658

returns 3 segments representing G-E, C, D-A
  $ mononoke_newadmin commit-graph -R repo segments --heads $G --verify
  9711852ec4f4b42937dd5b760c7b3f84345bf48c74b7ef3ca7118d1d7928744d -> f0c81a03319da010415f712831abe8469ba3c30b93b0b07af175302b8c15f0e6, length: 3, parents: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2 (e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2~0), 5a25c0a76794bbcc5180da0949a652750101597f0fbade488e611d5c0917e7be (5a25c0a76794bbcc5180da0949a652750101597f0fbade488e611d5c0917e7be~0)
  e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2 -> e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2, length: 1, parents: f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658 (5a25c0a76794bbcc5180da0949a652750101597f0fbade488e611d5c0917e7be~1)
  5a25c0a76794bbcc5180da0949a652750101597f0fbade488e611d5c0917e7be -> aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675, length: 3, parents: 
