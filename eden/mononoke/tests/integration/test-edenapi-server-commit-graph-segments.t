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

Ensure we can clone the repo
  $ cd $TESTTMP
  $ hgedenapi clone "mononoke://$(mononoke_address)/repo" repo-hg
  fetching lazy changelog
  populating main commit graph
  tip commit: 4e9f8e556b01de1ac058397e86387d37778808d2
  fetching selected remote bookmarks
  updating to branch default
  17 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd repo-hg

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
