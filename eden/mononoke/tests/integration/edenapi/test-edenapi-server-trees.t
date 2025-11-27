# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Set up local hgrc and Mononoke config.
  $ setup_common_config
  $ cd $TESTTMP

Initialize test repo.
  $ hginit_treemanifest repo
  $ cd repo
  $ testtool_drawdag -R repo --print-hg-hashes <<EOF
  > COMMIT_2
  > |
  > COMMIT_1
  > # modify: COMMIT_1 "test.txt" "test content\n"
  > # message: COMMIT_1 "add test.txt"
  > # copy: COMMIT_2 "copy.txt" "test content\n" "COMMIT_1" "test.txt"
  > # message: COMMIT_2 "copy test.txt to test2.txt"
  > EOF
  COMMIT_1=* (glob)
  COMMIT_2=* (glob)

Import test repo.
  $ cd ..

Start up SaplingRemoteAPI server.
  $ setup_mononoke_config
  $ SCUBA="$TESTTMP/scuba.json"
  $ start_and_wait_for_mononoke_server --scuba-log-file "$SCUBA"

Extract manifest IDs and commit hashes
  $ cd $TESTTMP/repo
  $ cat >> .hg/hgrc <<EOF
  > [paths]
  > default = mono:repo
  > EOF
  $ hg pull -q -r $COMMIT_1 -r $COMMIT_2
  $ ROOT_MFID_1=$(hg log -r $COMMIT_1 -T '{manifest}')
  $ ROOT_MFID_2=$(hg log -r $COMMIT_2 -T '{manifest}')
  $ HG_ID_1="$COMMIT_1"
  $ HG_ID_2="$COMMIT_2"
  $ cd $TESTTMP

Create and send tree request.
  $ cat > keys << EOF
  > [
  >     ("", "$ROOT_MFID_1"),
  >     ("", "$ROOT_MFID_2")
  > ]
  > EOF

  $ cat > attrs << EOF
  > {
  >     "manifest_blob": True,
  >     "parents": True,
  >     "child_metadata": True
  > }
  > EOF

  $ hg debugapi mono:repo -e trees -f keys -f attrs --sort
  [{"key": {"node": bin("b3930c8a2f6a25b56d20ed48ce1d30cd98026792"),
            "path": ""},
    "data": b"COMMIT_1\05690dd090bcba4b8f272493af3c574cd5242c4d1\ntest.txt\0186cafa3319c24956783383dc44c5cbc68c5a0ca\n",
    "parents": None,
    "children": [{"Ok": {"File": {"key": {"node": bin("5690dd090bcba4b8f272493af3c574cd5242c4d1"),
                                          "path": "COMMIT_1"},
                                  "file_metadata": {"size": 8,
                                                    "content_id": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "content_sha1": bin("caeda72b3c84736c17cc15dfd79bf5c3efa08c8c"),
                                                    "content_blake3": bin("f345aeb96f603ce728210cd8481f50d3f73679d206afbdaa5dc554c05f1501ae"),
                                                    "content_sha256": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "file_header_metadata": b""}}}},
                 {"Ok": {"File": {"key": {"node": bin("186cafa3319c24956783383dc44c5cbc68c5a0ca"),
                                          "path": "test.txt"},
                                  "file_metadata": {"size": 13,
                                                    "content_id": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "content_sha1": bin("4fe2b8dd12cd9cd6a413ea960cd8c09c25f19527"),
                                                    "content_blake3": bin("7e9a0ce0d68016f0502ac50ff401830c7e2e9c894b43b242439f90f99af8835a"),
                                                    "content_sha256": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "file_header_metadata": b""}}}}],
    "tree_aux_data": None},
   {"key": {"node": bin("dfe7fab71e1f96a0f0f53b0c76725c01d79b244d"),
            "path": ""},
    "data": b"COMMIT_1\05690dd090bcba4b8f272493af3c574cd5242c4d1\nCOMMIT_2\030a356c25fb06508d81ed1dceb0550bcaa1ba9e0\ncopy.txt\017b8d4e3bafd4ec4812ad7c930aace9bf07ab033\ntest.txt\0186cafa3319c24956783383dc44c5cbc68c5a0ca\n",
    "parents": bin("b3930c8a2f6a25b56d20ed48ce1d30cd98026792"),
    "children": [{"Ok": {"File": {"key": {"node": bin("5690dd090bcba4b8f272493af3c574cd5242c4d1"),
                                          "path": "COMMIT_1"},
                                  "file_metadata": {"size": 8,
                                                    "content_id": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "content_sha1": bin("caeda72b3c84736c17cc15dfd79bf5c3efa08c8c"),
                                                    "content_blake3": bin("f345aeb96f603ce728210cd8481f50d3f73679d206afbdaa5dc554c05f1501ae"),
                                                    "content_sha256": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "file_header_metadata": b""}}}},
                 {"Ok": {"File": {"key": {"node": bin("30a356c25fb06508d81ed1dceb0550bcaa1ba9e0"),
                                          "path": "COMMIT_2"},
                                  "file_metadata": {"size": 8,
                                                    "content_id": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "content_sha1": bin("0d5c8bf514dfef3ebe753eaf921d3eb63780d5dc"),
                                                    "content_blake3": bin("478f37d7d3cc01195aa96f425b687dc04fc038d565c08ad438516755d3396a63"),
                                                    "content_sha256": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "file_header_metadata": b""}}}},
                 {"Ok": {"File": {"key": {"node": bin("17b8d4e3bafd4ec4812ad7c930aace9bf07ab033"),
                                          "path": "copy.txt"},
                                  "file_metadata": {"size": 13,
                                                    "content_id": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "content_sha1": bin("4fe2b8dd12cd9cd6a413ea960cd8c09c25f19527"),
                                                    "content_blake3": bin("7e9a0ce0d68016f0502ac50ff401830c7e2e9c894b43b242439f90f99af8835a"),
                                                    "content_sha256": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "file_header_metadata": b"\x01\ncopy: test.txt\ncopyrev: 186cafa3319c24956783383dc44c5cbc68c5a0ca\n\x01\n"}}}},
                 {"Ok": {"File": {"key": {"node": bin("186cafa3319c24956783383dc44c5cbc68c5a0ca"),
                                          "path": "test.txt"},
                                  "file_metadata": {"size": 13,
                                                    "content_id": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "content_sha1": bin("4fe2b8dd12cd9cd6a413ea960cd8c09c25f19527"),
                                                    "content_blake3": bin("7e9a0ce0d68016f0502ac50ff401830c7e2e9c894b43b242439f90f99af8835a"),
                                                    "content_sha256": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "file_header_metadata": b""}}}}],
    "tree_aux_data": None}]

  $ cat > attrs << EOF
  > {
  >     "manifest_blob": True,
  >     "parents": True,
  >     "child_metadata": True,
  >     "augmented_trees": True
  > }
  > EOF

Expected fallback (tree_aux_data is not returned)
  $ hg debugapi mono:repo -e trees -f keys -f attrs --sort
  [{"key": {"node": bin("b3930c8a2f6a25b56d20ed48ce1d30cd98026792"),
            "path": ""},
    "data": b"COMMIT_1\05690dd090bcba4b8f272493af3c574cd5242c4d1\ntest.txt\0186cafa3319c24956783383dc44c5cbc68c5a0ca\n",
    "parents": None,
    "children": [{"Ok": {"File": {"key": {"node": bin("5690dd090bcba4b8f272493af3c574cd5242c4d1"),
                                          "path": "COMMIT_1"},
                                  "file_metadata": {"size": 8,
                                                    "content_id": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "content_sha1": bin("caeda72b3c84736c17cc15dfd79bf5c3efa08c8c"),
                                                    "content_blake3": bin("f345aeb96f603ce728210cd8481f50d3f73679d206afbdaa5dc554c05f1501ae"),
                                                    "content_sha256": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "file_header_metadata": b""}}}},
                 {"Ok": {"File": {"key": {"node": bin("186cafa3319c24956783383dc44c5cbc68c5a0ca"),
                                          "path": "test.txt"},
                                  "file_metadata": {"size": 13,
                                                    "content_id": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "content_sha1": bin("4fe2b8dd12cd9cd6a413ea960cd8c09c25f19527"),
                                                    "content_blake3": bin("7e9a0ce0d68016f0502ac50ff401830c7e2e9c894b43b242439f90f99af8835a"),
                                                    "content_sha256": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "file_header_metadata": b""}}}}],
    "tree_aux_data": None},
   {"key": {"node": bin("dfe7fab71e1f96a0f0f53b0c76725c01d79b244d"),
            "path": ""},
    "data": b"COMMIT_1\05690dd090bcba4b8f272493af3c574cd5242c4d1\nCOMMIT_2\030a356c25fb06508d81ed1dceb0550bcaa1ba9e0\ncopy.txt\017b8d4e3bafd4ec4812ad7c930aace9bf07ab033\ntest.txt\0186cafa3319c24956783383dc44c5cbc68c5a0ca\n",
    "parents": bin("b3930c8a2f6a25b56d20ed48ce1d30cd98026792"),
    "children": [{"Ok": {"File": {"key": {"node": bin("5690dd090bcba4b8f272493af3c574cd5242c4d1"),
                                          "path": "COMMIT_1"},
                                  "file_metadata": {"size": 8,
                                                    "content_id": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "content_sha1": bin("caeda72b3c84736c17cc15dfd79bf5c3efa08c8c"),
                                                    "content_blake3": bin("f345aeb96f603ce728210cd8481f50d3f73679d206afbdaa5dc554c05f1501ae"),
                                                    "content_sha256": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "file_header_metadata": b""}}}},
                 {"Ok": {"File": {"key": {"node": bin("30a356c25fb06508d81ed1dceb0550bcaa1ba9e0"),
                                          "path": "COMMIT_2"},
                                  "file_metadata": {"size": 8,
                                                    "content_id": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "content_sha1": bin("0d5c8bf514dfef3ebe753eaf921d3eb63780d5dc"),
                                                    "content_blake3": bin("478f37d7d3cc01195aa96f425b687dc04fc038d565c08ad438516755d3396a63"),
                                                    "content_sha256": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "file_header_metadata": b""}}}},
                 {"Ok": {"File": {"key": {"node": bin("17b8d4e3bafd4ec4812ad7c930aace9bf07ab033"),
                                          "path": "copy.txt"},
                                  "file_metadata": {"size": 13,
                                                    "content_id": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "content_sha1": bin("4fe2b8dd12cd9cd6a413ea960cd8c09c25f19527"),
                                                    "content_blake3": bin("7e9a0ce0d68016f0502ac50ff401830c7e2e9c894b43b242439f90f99af8835a"),
                                                    "content_sha256": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "file_header_metadata": b"\x01\ncopy: test.txt\ncopyrev: 186cafa3319c24956783383dc44c5cbc68c5a0ca\n\x01\n"}}}},
                 {"Ok": {"File": {"key": {"node": bin("186cafa3319c24956783383dc44c5cbc68c5a0ca"),
                                          "path": "test.txt"},
                                  "file_metadata": {"size": 13,
                                                    "content_id": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "content_sha1": bin("4fe2b8dd12cd9cd6a413ea960cd8c09c25f19527"),
                                                    "content_blake3": bin("7e9a0ce0d68016f0502ac50ff401830c7e2e9c894b43b242439f90f99af8835a"),
                                                    "content_sha256": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "file_header_metadata": b""}}}}],
    "tree_aux_data": None}]

Expected for tree_aux_data to be returned.
  $ mononoke_admin derived-data -R repo derive --derived-data-types hg_augmented_manifests -i $HG_ID_1 -i $HG_ID_2 --from-predecessor
  $ hg debugapi mono:repo -e trees -f keys -f attrs --sort
  [{"key": {"node": bin("b3930c8a2f6a25b56d20ed48ce1d30cd98026792"),
            "path": ""},
    "data": b"COMMIT_1\05690dd090bcba4b8f272493af3c574cd5242c4d1\ntest.txt\0186cafa3319c24956783383dc44c5cbc68c5a0ca\n",
    "parents": None,
    "children": [{"Ok": {"File": {"key": {"node": bin("5690dd090bcba4b8f272493af3c574cd5242c4d1"),
                                          "path": "COMMIT_1"},
                                  "file_metadata": {"size": 8,
                                                    "content_id": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "content_sha1": bin("caeda72b3c84736c17cc15dfd79bf5c3efa08c8c"),
                                                    "content_blake3": bin("f345aeb96f603ce728210cd8481f50d3f73679d206afbdaa5dc554c05f1501ae"),
                                                    "content_sha256": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "file_header_metadata": b""}}}},
                 {"Ok": {"File": {"key": {"node": bin("186cafa3319c24956783383dc44c5cbc68c5a0ca"),
                                          "path": "test.txt"},
                                  "file_metadata": {"size": 13,
                                                    "content_id": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "content_sha1": bin("4fe2b8dd12cd9cd6a413ea960cd8c09c25f19527"),
                                                    "content_blake3": bin("7e9a0ce0d68016f0502ac50ff401830c7e2e9c894b43b242439f90f99af8835a"),
                                                    "content_sha256": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "file_header_metadata": b""}}}}],
    "tree_aux_data": {"augmented_manifest_id": bin("3fa0541cdf451ab034a94a5006399bde88da365896ed364798106c5757248d41"),
                      "augmented_manifest_size": 373}},
   {"key": {"node": bin("dfe7fab71e1f96a0f0f53b0c76725c01d79b244d"),
            "path": ""},
    "data": b"COMMIT_1\05690dd090bcba4b8f272493af3c574cd5242c4d1\nCOMMIT_2\030a356c25fb06508d81ed1dceb0550bcaa1ba9e0\ncopy.txt\017b8d4e3bafd4ec4812ad7c930aace9bf07ab033\ntest.txt\0186cafa3319c24956783383dc44c5cbc68c5a0ca\n",
    "parents": bin("b3930c8a2f6a25b56d20ed48ce1d30cd98026792"),
    "children": [{"Ok": {"File": {"key": {"node": bin("5690dd090bcba4b8f272493af3c574cd5242c4d1"),
                                          "path": "COMMIT_1"},
                                  "file_metadata": {"size": 8,
                                                    "content_id": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "content_sha1": bin("caeda72b3c84736c17cc15dfd79bf5c3efa08c8c"),
                                                    "content_blake3": bin("f345aeb96f603ce728210cd8481f50d3f73679d206afbdaa5dc554c05f1501ae"),
                                                    "content_sha256": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "file_header_metadata": b""}}}},
                 {"Ok": {"File": {"key": {"node": bin("30a356c25fb06508d81ed1dceb0550bcaa1ba9e0"),
                                          "path": "COMMIT_2"},
                                  "file_metadata": {"size": 8,
                                                    "content_id": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "content_sha1": bin("0d5c8bf514dfef3ebe753eaf921d3eb63780d5dc"),
                                                    "content_blake3": bin("478f37d7d3cc01195aa96f425b687dc04fc038d565c08ad438516755d3396a63"),
                                                    "content_sha256": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "file_header_metadata": b""}}}},
                 {"Ok": {"File": {"key": {"node": bin("17b8d4e3bafd4ec4812ad7c930aace9bf07ab033"),
                                          "path": "copy.txt"},
                                  "file_metadata": {"size": 13,
                                                    "content_id": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "content_sha1": bin("4fe2b8dd12cd9cd6a413ea960cd8c09c25f19527"),
                                                    "content_blake3": bin("7e9a0ce0d68016f0502ac50ff401830c7e2e9c894b43b242439f90f99af8835a"),
                                                    "content_sha256": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "file_header_metadata": b"\x01\ncopy: test.txt\ncopyrev: 186cafa3319c24956783383dc44c5cbc68c5a0ca\n\x01\n"}}}},
                 {"Ok": {"File": {"key": {"node": bin("186cafa3319c24956783383dc44c5cbc68c5a0ca"),
                                          "path": "test.txt"},
                                  "file_metadata": {"size": 13,
                                                    "content_id": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "content_sha1": bin("4fe2b8dd12cd9cd6a413ea960cd8c09c25f19527"),
                                                    "content_blake3": bin("7e9a0ce0d68016f0502ac50ff401830c7e2e9c894b43b242439f90f99af8835a"),
                                                    "content_sha256": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "file_header_metadata": b""}}}}],
    "tree_aux_data": {"augmented_manifest_id": bin("0692535594cfc449a55b4c4b1a0c2ce290dd6bae40e188805f97ff6b9e88b79d"),
                      "augmented_manifest_size": 826}}]

  $ cat "$SCUBA" | jq '. | select(.normal.log_tag == "EdenAPI Request Processed" and .normal.edenapi_method == "trees") | {edenapi_method: .normal.edenapi_method, fetch_from_cas_attempted: .normal.fetch_from_cas_attempted}' | jq -s '.[0]'
  {
    "edenapi_method": "trees",
    "fetch_from_cas_attempted": "false"
  }
