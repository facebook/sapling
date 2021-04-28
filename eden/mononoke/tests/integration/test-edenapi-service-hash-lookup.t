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
  $ hg log -G -r "all()" -T "{desc} {node}\n"
  o  F 11abe3fb10b8689b560681094b17fe161871d043
  │
  │ o  D f585351a92f85104bff7c284233c338b10eb1df7
  │ │
  o │  E 49cb92066bfd0763fff729c354345650b7428554
  │ │
  │ o  C 26805aba1e600a82e93661149f2313866a221a7b
  ├─╯
  o  B 112478962961147124edd43549aedd1a335e44bf
  │
  o  A 426bada5c67598ca65036d57d9e4b64b0c1ce7a0
  

Import and start mononoke
  $ cd ..
  $ blobimport repo-hg/.hg repo
  $ SEGMENTED_CHANGELOG_ENABLE=1 setup_mononoke_config
  $ mononoke
  $ wait_for_mononoke


Create and send file data request.
  $ edenapi_make_req commit-hash-lookup > req.cbor <<EOF
  > {
  >   "batch": [{
  >     "inclusive_range": {
  >       "low": "$A",
  >       "high": "$D"
  >     }
  >   }, {
  >     "inclusive_range": {
  >       "low": "$C",
  >       "high": "$C"
  >     }
  >   }, {
  >     "inclusive_range": {
  >       "low": "0000000000000000000000000000000000000000",
  >       "high": "ffffffffffffffffffffffffffffffffffffffff"
  >     }
  >   }, {
  >     "inclusive_range": {
  >       "low": "ffffffffffffffffffffffffffffffffffffffff",
  >       "high": "ffffffffffffffffffffffffffffffffffffffff"
  >     }
  >   }]
  > }
  > EOF
  Reading from stdin
  Generated request: WireBatch {
      batch: [
          WireCommitHashLookupRequest {
              inclusive_range: Some(
                  (
                      WireHgId("426bada5c67598ca65036d57d9e4b64b0c1ce7a0"),
                      WireHgId("f585351a92f85104bff7c284233c338b10eb1df7"),
                  ),
              ),
          },
          WireCommitHashLookupRequest {
              inclusive_range: Some(
                  (
                      WireHgId("26805aba1e600a82e93661149f2313866a221a7b"),
                      WireHgId("26805aba1e600a82e93661149f2313866a221a7b"),
                  ),
              ),
          },
          WireCommitHashLookupRequest {
              inclusive_range: Some(
                  (
                      WireHgId("0000000000000000000000000000000000000000"),
                      WireHgId("ffffffffffffffffffffffffffffffffffffffff"),
                  ),
              ),
          },
          WireCommitHashLookupRequest {
              inclusive_range: Some(
                  (
                      WireHgId("ffffffffffffffffffffffffffffffffffffffff"),
                      WireHgId("ffffffffffffffffffffffffffffffffffffffff"),
                  ),
              ),
          },
      ],
  }

  $ sslcurl -s "https://localhost:$MONONOKE_SOCKET/edenapi/repo/commit/hash_lookup" --data-binary @req.cbor > res.cbor

Check files in response.
  $ edenapi_read_res commit-hash-lookup res.cbor
  Reading from file: "res.cbor"
  InclusiveRange(426bada5c67598ca65036d57d9e4b64b0c1ce7a0, f585351a92f85104bff7c284233c338b10eb1df7)
    [426bada5c67598ca65036d57d9e4b64b0c1ce7a0, 49cb92066bfd0763fff729c354345650b7428554, f585351a92f85104bff7c284233c338b10eb1df7]
  InclusiveRange(26805aba1e600a82e93661149f2313866a221a7b, 26805aba1e600a82e93661149f2313866a221a7b)
    [26805aba1e600a82e93661149f2313866a221a7b]
  InclusiveRange(0000000000000000000000000000000000000000, ffffffffffffffffffffffffffffffffffffffff)
    [112478962961147124edd43549aedd1a335e44bf, 11abe3fb10b8689b560681094b17fe161871d043, 26805aba1e600a82e93661149f2313866a221a7b, 426bada5c67598ca65036d57d9e4b64b0c1ce7a0, 49cb92066bfd0763fff729c354345650b7428554, f585351a92f85104bff7c284233c338b10eb1df7]
  InclusiveRange(ffffffffffffffffffffffffffffffffffffffff, ffffffffffffffffffffffffffffffffffffffff)
    []
