# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ setup_common_config
  $ configure modern
  $ setconfig remotenames.selectivepull=True remotenames.selectivepulldefault=master_bookmark
  $ setconfig devel.segmented-changelog-rev-compat=False

  $ testtool_drawdag -R repo --derive-all --print-hg-hashes <<'EOF'
  > A-B-C-D-G-M-N-O-P-Q
  >    \   /   / /
  >     E-F-K-L /
  >      \     /
  >       H-I-J
  > # bookmark: Q master_bookmark
  > EOF
  A=20ca2a4749a439b459125ef0f6a4f26e88ee7538
  B=80521a640a0c8f51dcc128c2658b224d595840ac
  C=d3b399ca8757acdb81c3681b052eb978db6768d8
  D=74dbcd84493ad579ee26bb326c4272983098f69c
  E=a66a30bed387971d9b4505eff1d9599dc16c141a
  F=d6e9a5359dcbb3b00616ebba901199b45d039851
  G=d09c6f0ee66af0b71738d48baecc9868dead4ae9
  H=26641f81ab7fd36bffbab851fc5ec16c3c2ec909
  I=163d712b1fc1272ff20d4b4f1781a3a953fdbd11
  J=abdf5b2a1b925a020e3271d37ec1a6d04d7f0130
  K=7d1d79d931b8753478bd5b2952e7aa53b095840c
  L=6a93301afe75bbd33ccccc884a669f74c27e6d9f
  M=1aaa5bb98ca42556993537d59e6db196618a6b4d
  N=0bfd89d079c2ea32dd4669543bf9e5a7b11e2d42
  O=705906cc9558bdb08dc5847424b6125c00a01c0f
  P=a050a5556469b55ca00d97899b06995b44569989
  Q=4e9f8e556b01de1ac058397e86387d37778808d2

Since hash-to-location and location-to-hash work via segmented changelog, we must still build one.
  $ quiet segmented_changelog_tailer_reseed --repo repo --head=master_bookmark

Enable Segmented Changelog
  $ cat >> "$TESTTMP/mononoke-config/repos/repo/server.toml" <<CONFIG
  > [segmented_changelog_config]
  > enabled=true
  > heads_to_include = [{ bookmark = "master_bookmark" }]
  > CONFIG

  $ start_and_wait_for_mononoke_server

Ensure we can clone the repo using the commit graph segments endpoint
  $ cd $TESTTMP
  $ hgedenapi clone "mononoke://$(mononoke_address)/repo" repo-hg --config clone.use-rust=true --config clone.use-commit-graph=true
  Cloning repo into $TESTTMP/repo-hg
  Checking out 'master_bookmark'
  17 files updated
  $ cd repo-hg
  $ hgedenapi log -G -T '{node|short} {desc}'
  @  4e9f8e556b01 Q
  │
  o  a050a5556469 P
  │
  o    705906cc9558 O
  ├─╮
  │ o    0bfd89d079c2 N
  │ ├─╮
  │ │ o  1aaa5bb98ca4 M
  │ │ │
  │ │ o    d09c6f0ee66a G
  │ │ ├─╮
  │ │ │ o  74dbcd84493a D
  │ │ │ │
  │ │ │ o  d3b399ca8757 C
  │ │ │ │
  │ o │ │  6a93301afe75 L
  │ │ │ │
  │ o │ │  7d1d79d931b8 K
  │ ├─╯ │
  │ o   │  d6e9a5359dcb F
  │ │   │
  o │   │  abdf5b2a1b92 J
  │ │   │
  o │   │  163d712b1fc1 I
  │ │   │
  o │   │  26641f81ab7f H
  ├─╯   │
  o     │  a66a30bed387 E
  ├─────╯
  o  80521a640a0c B
  │
  o  20ca2a4749a4 A
  
  $ hgedenapi log -r tip
  commit:      4e9f8e556b01
  bookmark:    remote/master_bookmark
  hoistedname: master_bookmark
  user:        author
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Q
  

  $ hgedenapi debugapi -e commitgraphsegments -i "[\"$Q\"]" -i "[]"
  [{"base": bin("705906cc9558bdb08dc5847424b6125c00a01c0f"),
    "head": bin("4e9f8e556b01de1ac058397e86387d37778808d2"),
    "length": 3,
    "parents": [{"hgid": bin("abdf5b2a1b925a020e3271d37ec1a6d04d7f0130"),
                 "location": {"distance": 0,
                              "descendant": bin("abdf5b2a1b925a020e3271d37ec1a6d04d7f0130")}},
                {"hgid": bin("0bfd89d079c2ea32dd4669543bf9e5a7b11e2d42"),
                 "location": {"distance": 0,
                              "descendant": bin("0bfd89d079c2ea32dd4669543bf9e5a7b11e2d42")}}]},
   {"base": bin("0bfd89d079c2ea32dd4669543bf9e5a7b11e2d42"),
    "head": bin("0bfd89d079c2ea32dd4669543bf9e5a7b11e2d42"),
    "length": 1,
    "parents": [{"hgid": bin("6a93301afe75bbd33ccccc884a669f74c27e6d9f"),
                 "location": {"distance": 0,
                              "descendant": bin("6a93301afe75bbd33ccccc884a669f74c27e6d9f")}},
                {"hgid": bin("1aaa5bb98ca42556993537d59e6db196618a6b4d"),
                 "location": {"distance": 0,
                              "descendant": bin("1aaa5bb98ca42556993537d59e6db196618a6b4d")}}]},
   {"base": bin("d09c6f0ee66af0b71738d48baecc9868dead4ae9"),
    "head": bin("1aaa5bb98ca42556993537d59e6db196618a6b4d"),
    "length": 2,
    "parents": [{"hgid": bin("74dbcd84493ad579ee26bb326c4272983098f69c"),
                 "location": {"distance": 0,
                              "descendant": bin("74dbcd84493ad579ee26bb326c4272983098f69c")}},
                {"hgid": bin("d6e9a5359dcbb3b00616ebba901199b45d039851"),
                 "location": {"distance": 2,
                              "descendant": bin("6a93301afe75bbd33ccccc884a669f74c27e6d9f")}}]},
   {"base": bin("26641f81ab7fd36bffbab851fc5ec16c3c2ec909"),
    "head": bin("abdf5b2a1b925a020e3271d37ec1a6d04d7f0130"),
    "length": 3,
    "parents": [{"hgid": bin("a66a30bed387971d9b4505eff1d9599dc16c141a"),
                 "location": {"distance": 3,
                              "descendant": bin("6a93301afe75bbd33ccccc884a669f74c27e6d9f")}}]},
   {"base": bin("d3b399ca8757acdb81c3681b052eb978db6768d8"),
    "head": bin("74dbcd84493ad579ee26bb326c4272983098f69c"),
    "length": 2,
    "parents": [{"hgid": bin("80521a640a0c8f51dcc128c2658b224d595840ac"),
                 "location": {"distance": 4,
                              "descendant": bin("6a93301afe75bbd33ccccc884a669f74c27e6d9f")}}]},
   {"base": bin("20ca2a4749a439b459125ef0f6a4f26e88ee7538"),
    "head": bin("6a93301afe75bbd33ccccc884a669f74c27e6d9f"),
    "length": 6,
    "parents": []}]

  $ hgedenapi debugapi -e commitgraphsegments -i "[\"$Q\"]" -i "[\"$G\"]"
  [{"base": bin("705906cc9558bdb08dc5847424b6125c00a01c0f"),
    "head": bin("4e9f8e556b01de1ac058397e86387d37778808d2"),
    "length": 3,
    "parents": [{"hgid": bin("abdf5b2a1b925a020e3271d37ec1a6d04d7f0130"),
                 "location": {"distance": 0,
                              "descendant": bin("abdf5b2a1b925a020e3271d37ec1a6d04d7f0130")}},
                {"hgid": bin("0bfd89d079c2ea32dd4669543bf9e5a7b11e2d42"),
                 "location": {"distance": 0,
                              "descendant": bin("0bfd89d079c2ea32dd4669543bf9e5a7b11e2d42")}}]},
   {"base": bin("0bfd89d079c2ea32dd4669543bf9e5a7b11e2d42"),
    "head": bin("0bfd89d079c2ea32dd4669543bf9e5a7b11e2d42"),
    "length": 1,
    "parents": [{"hgid": bin("6a93301afe75bbd33ccccc884a669f74c27e6d9f"),
                 "location": {"distance": 0,
                              "descendant": bin("6a93301afe75bbd33ccccc884a669f74c27e6d9f")}},
                {"hgid": bin("1aaa5bb98ca42556993537d59e6db196618a6b4d"),
                 "location": {"distance": 0,
                              "descendant": bin("1aaa5bb98ca42556993537d59e6db196618a6b4d")}}]},
   {"base": bin("1aaa5bb98ca42556993537d59e6db196618a6b4d"),
    "head": bin("1aaa5bb98ca42556993537d59e6db196618a6b4d"),
    "length": 1,
    "parents": [{"hgid": bin("d09c6f0ee66af0b71738d48baecc9868dead4ae9"),
                 "location": None}]},
   {"base": bin("7d1d79d931b8753478bd5b2952e7aa53b095840c"),
    "head": bin("6a93301afe75bbd33ccccc884a669f74c27e6d9f"),
    "length": 2,
    "parents": [{"hgid": bin("d6e9a5359dcbb3b00616ebba901199b45d039851"),
                 "location": None}]},
   {"base": bin("26641f81ab7fd36bffbab851fc5ec16c3c2ec909"),
    "head": bin("abdf5b2a1b925a020e3271d37ec1a6d04d7f0130"),
    "length": 3,
    "parents": [{"hgid": bin("a66a30bed387971d9b4505eff1d9599dc16c141a"),
                 "location": None}]}]

Add some new commits, move the master bookmark and do a pull

  $ testtool_drawdag -R repo --derive-all --print-hg-hashes <<'EOF'
  > Q-R-S-T-W-X
  >    \   /
  >     U-V
  > # exists: Q 2f7f5cd90b7b58f14d6b20b83b95478d5b0ab8c1e5bf429bc317256813516895
  > EOF
  Q=4e9f8e556b01de1ac058397e86387d37778808d2
  R=abaf42d51402beae9b140f8eb367d454f884e7a8
  S=8036723dc80d2be07ce5da923313fe7c4179b803
  T=cc84bc524e272bf11d5901ff6fcdbdb3b3dc0e11
  U=f4c65fda5311a2cea174f2a03f24db5773500996
  V=f42c7fb75580709fa1bf5f6e75efdb74338747e0
  W=d728ac072e7662d3ce354b3a13dbd7d3ca853591
  X=262080717826d7e3df89d22278d80710f78457e6

  $ mononoke_newadmin bookmarks -R repo set master_bookmark "$X"
  Updating publishing bookmark master_bookmark from 2f7f5cd90b7b58f14d6b20b83b95478d5b0ab8c1e5bf429bc317256813516895 to e4b2425dd7affae8fd14348623eb66fa78e9a7ead9330b203d828dae0ec19f79

Since hash-to-location is still using the server-side segmented changelog, we must make sure it's up-to-date.
  $ quiet segmented_changelog_tailer_once --repo repo

  $ flush_mononoke_bookmarks
  $ sleep 1

  $ hgedenapi pull --config pull.use-commit-graph=true
  pulling from mononoke://$LOCALIP:$LOCAL_PORT/repo
  imported commit graph for 7 commits (3 segments)

  $ hgedenapi log -G -T '{node|short} {desc}'
  o  262080717826 X
  │
  o    d728ac072e76 W
  ├─╮
  │ o  f42c7fb75580 V
  │ │
  │ o  f4c65fda5311 U
  │ │
  o │  cc84bc524e27 T
  │ │
  o │  8036723dc80d S
  ├─╯
  o  abaf42d51402 R
  │
  @  4e9f8e556b01 Q
  │
  o  a050a5556469 P
  │
  o    705906cc9558 O
  ├─╮
  │ o    0bfd89d079c2 N
  │ ├─╮
  │ │ o  1aaa5bb98ca4 M
  │ │ │
  │ │ o    d09c6f0ee66a G
  │ │ ├─╮
  │ │ │ o  74dbcd84493a D
  │ │ │ │
  │ │ │ o  d3b399ca8757 C
  │ │ │ │
  │ o │ │  6a93301afe75 L
  │ │ │ │
  │ o │ │  7d1d79d931b8 K
  │ ├─╯ │
  │ o   │  d6e9a5359dcb F
  │ │   │
  o │   │  abdf5b2a1b92 J
  │ │   │
  o │   │  163d712b1fc1 I
  │ │   │
  o │   │  26641f81ab7f H
  ├─╯   │
  o     │  a66a30bed387 E
  ├─────╯
  o  80521a640a0c B
  │
  o  20ca2a4749a4 A
  

  $ hgedenapi log -r tip
  commit:      4e9f8e556b01
  user:        author
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Q
  

  $ hgedenapi debugapi -e commitgraphsegments -i "[\"$X\"]" -i "[\"$Q\"]"
  [{"base": bin("d728ac072e7662d3ce354b3a13dbd7d3ca853591"),
    "head": bin("262080717826d7e3df89d22278d80710f78457e6"),
    "length": 2,
    "parents": [{"hgid": bin("cc84bc524e272bf11d5901ff6fcdbdb3b3dc0e11"),
                 "location": {"distance": 0,
                              "descendant": bin("cc84bc524e272bf11d5901ff6fcdbdb3b3dc0e11")}},
                {"hgid": bin("f42c7fb75580709fa1bf5f6e75efdb74338747e0"),
                 "location": {"distance": 0,
                              "descendant": bin("f42c7fb75580709fa1bf5f6e75efdb74338747e0")}}]},
   {"base": bin("8036723dc80d2be07ce5da923313fe7c4179b803"),
    "head": bin("cc84bc524e272bf11d5901ff6fcdbdb3b3dc0e11"),
    "length": 2,
    "parents": [{"hgid": bin("abaf42d51402beae9b140f8eb367d454f884e7a8"),
                 "location": {"distance": 2,
                              "descendant": bin("f42c7fb75580709fa1bf5f6e75efdb74338747e0")}}]},
   {"base": bin("abaf42d51402beae9b140f8eb367d454f884e7a8"),
    "head": bin("f42c7fb75580709fa1bf5f6e75efdb74338747e0"),
    "length": 3,
    "parents": [{"hgid": bin("4e9f8e556b01de1ac058397e86387d37778808d2"),
                 "location": None}]}]


