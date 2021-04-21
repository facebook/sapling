# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Set up local hgrc and Mononoke config.
  $ setup_common_config
  $ setup_configerator_configs
  $ cd $TESTTMP

Initialize test repo.
  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ setup_hg_server

Populate test repo
  $ drawdag << EOS
  >   F
  >   |
  > D |
  > | E
  > C |
  >  \|
  >   B
  >   |
  >   A
  > EOS
  $ hg bookmark -r "$F" "master_bookmark"
  $ log -r "all()"
  o  F [draft;rev=5;11abe3fb10b8]
  │
  │ o  D [draft;rev=4;f585351a92f8]
  │ │
  o │  E [draft;rev=3;49cb92066bfd]
  │ │
  │ o  C [draft;rev=2;26805aba1e60]
  ├─╯
  o  B [draft;rev=1;112478962961]
  │
  o  A [draft;rev=0;426bada5c675]
  $

Import and start mononoke
  $ cd ..
  $ blobimport repo-hg/.hg repo
  $ SEGMENTED_CHANGELOG_ENABLE=1 setup_mononoke_config
  $ mononoke
  $ wait_for_mononoke


Create and send file data request.
  $ edenapi_make_req commit-hash-to-location > req.cbor <<EOF
  > {
  >   "master_heads": [
  >     "$D",
  >     "$E"
  >   ],
  >   "hgids": [
  >     "$B",
  >     "$C"
  >   ],
  >   "unfiltered": true
  > }
  > EOF
  Reading from stdin
  Generated request: WireCommitHashToLocationRequestBatch {
      client_head: Some(
          WireHgId("f585351a92f85104bff7c284233c338b10eb1df7"),
      ),
      hgids: [
          WireHgId("112478962961147124edd43549aedd1a335e44bf"),
          WireHgId("26805aba1e600a82e93661149f2313866a221a7b"),
      ],
      master_heads: [
          WireHgId("f585351a92f85104bff7c284233c338b10eb1df7"),
          WireHgId("49cb92066bfd0763fff729c354345650b7428554"),
      ],
      unfiltered: Some(
          true,
      ),
  }

  $ sslcurl -s "https://localhost:$MONONOKE_SOCKET/edenapi/repo/commit/hash_to_location" --data-binary @req.cbor > res.cbor

Check files in response.
  $ edenapi_read_res commit-hash-to-location res.cbor
  Reading from file: "res.cbor"
  112478962961147124edd43549aedd1a335e44bf =>
      Err(code=1, msg='InternalError(InternalError(error while getting an up to date dag')
  26805aba1e600a82e93661149f2313866a221a7b =>
      Err(code=1, msg='InternalError(InternalError(error while getting an up to date dag')
