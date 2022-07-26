# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config

  $ testtool_drawdag -R repo << 'EOF'
  > J G
  > | |
  > I F
  > | |
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

  $ mononoke_newadmin changelog -R repo graph -i $G,$J -M -I
  o  message: G, id: 9711852ec4f4b42937dd5b760c7b3f84345bf48c74b7ef3ca7118d1d7928744d
  │
  │ o  message: J, id: 55e5dbaa7f26e0cfa1c2ee95479e2af088bf81caae4c2356d6eb8dfa6c114284
  │ │
  o │  message: F, id: 48779d8d497815015031dc3f3e9888abc8cf8273184ebd9ca8a395e24d501c90
  │ │
  │ o  message: I, id: 03ffabc887d3d9a81be514037b1dfa3020466af9145bafbc33a8880fd8808c01
  │ │
  o │    message: E, id: f0c81a03319da010415f712831abe8469ba3c30b93b0b07af175302b8c15f0e6
  ├───╮
  │ o │  message: H, id: 64642fc5a09343c1699e2ecaa5fa1c31fdb19f1e125428cd745327911c0b1d83
  ├─╯ │
  o   │  message: C, id: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  │   │
  │   o  message: D, id: 5a25c0a76794bbcc5180da0949a652750101597f0fbade488e611d5c0917e7be
  ├───╯
  o  message: B, id: f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  │
  o  message: A, id: aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675

  $ mononoke_newadmin changelog -R repo graph -i $G,$J -l 3 -M -I
  o  message: G, id: 9711852ec4f4b42937dd5b760c7b3f84345bf48c74b7ef3ca7118d1d7928744d
  │
  │ o  message: J, id: 55e5dbaa7f26e0cfa1c2ee95479e2af088bf81caae4c2356d6eb8dfa6c114284
  │ │
  o │  message: F, id: 48779d8d497815015031dc3f3e9888abc8cf8273184ebd9ca8a395e24d501c90
  │ │
  │ o  message: I, id: 03ffabc887d3d9a81be514037b1dfa3020466af9145bafbc33a8880fd8808c01
  │ │
  o │    message: E, id: f0c81a03319da010415f712831abe8469ba3c30b93b0b07af175302b8c15f0e6
  ├───╮
  │ o │  message: H, id: 64642fc5a09343c1699e2ecaa5fa1c31fdb19f1e125428cd745327911c0b1d83
  ├─╯ │
  o   │  message: C, id: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  │   │
  │   o  message: D, id: 5a25c0a76794bbcc5180da0949a652750101597f0fbade488e611d5c0917e7be
  ├───╯
  ~

  $ mononoke_newadmin changelog -R repo graph -i $C,$D,$G,$J -l 1 -M -I
  o  message: G, id: 9711852ec4f4b42937dd5b760c7b3f84345bf48c74b7ef3ca7118d1d7928744d
  │
  │ o  message: J, id: 55e5dbaa7f26e0cfa1c2ee95479e2af088bf81caae4c2356d6eb8dfa6c114284
  │ │
  o │  message: F, id: 48779d8d497815015031dc3f3e9888abc8cf8273184ebd9ca8a395e24d501c90
  │ │
  │ o  message: I, id: 03ffabc887d3d9a81be514037b1dfa3020466af9145bafbc33a8880fd8808c01
  │ │
  ~ │
    │
    ~
  o  message: C, id: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  │
  │ o  message: D, id: 5a25c0a76794bbcc5180da0949a652750101597f0fbade488e611d5c0917e7be
  ├─╯
  o  message: B, id: f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  │
  ~

  $ mononoke_newadmin changelog -R repo graph -i $G,$J -M -I -A -D
  o  message: G, id: 9711852ec4f4b42937dd5b760c7b3f84345bf48c74b7ef3ca7118d1d7928744d, author: author, author date: 1970-01-01 00:00:00 +00:00
  │
  │ o  message: J, id: 55e5dbaa7f26e0cfa1c2ee95479e2af088bf81caae4c2356d6eb8dfa6c114284, author: author, author date: 1970-01-01 00:00:00 +00:00
  │ │
  o │  message: F, id: 48779d8d497815015031dc3f3e9888abc8cf8273184ebd9ca8a395e24d501c90, author: author, author date: 1970-01-01 00:00:00 +00:00
  │ │
  │ o  message: I, id: 03ffabc887d3d9a81be514037b1dfa3020466af9145bafbc33a8880fd8808c01, author: author, author date: 1970-01-01 00:00:00 +00:00
  │ │
  o │    message: E, id: f0c81a03319da010415f712831abe8469ba3c30b93b0b07af175302b8c15f0e6, author: author, author date: 1970-01-01 00:00:00 +00:00
  ├───╮
  │ o │  message: H, id: 64642fc5a09343c1699e2ecaa5fa1c31fdb19f1e125428cd745327911c0b1d83, author: author, author date: 1970-01-01 00:00:00 +00:00
  ├─╯ │
  o   │  message: C, id: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2, author: author, author date: 1970-01-01 00:00:00 +00:00
  │   │
  │   o  message: D, id: 5a25c0a76794bbcc5180da0949a652750101597f0fbade488e611d5c0917e7be, author: author, author date: 1970-01-01 00:00:00 +00:00
  ├───╯
  o  message: B, id: f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658, author: author, author date: 1970-01-01 00:00:00 +00:00
  │
  o  message: A, id: aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675, author: author, author date: 1970-01-01 00:00:00 +00:00
