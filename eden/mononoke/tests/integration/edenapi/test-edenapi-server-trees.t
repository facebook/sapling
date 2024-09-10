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

Populate test repo
  $ echo "test content" > test.txt
  $ hg commit -Aqm "add test.txt"
  $ ROOT_MFID_1=$(hg log -r . -T '{manifest}')
  $ HG_ID_1=$(hg log -r . -T '{node}')
  $ hg cp test.txt copy.txt
  $ hg commit -Aqm "copy test.txt to test2.txt"
  $ ROOT_MFID_2=$(hg log -r . -T '{manifest}')
  $ HG_ID_2=$(hg log -r . -T '{node}')

Blobimport test repo.
  $ cd ..
  $ blobimport repo/.hg repo

Start up SaplingRemoteAPI server.
  $ start_and_wait_for_mononoke_server
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
  [{"key": {"node": bin("15024c4dc4a27b572d623db342ae6a08d7f7adec"),
            "path": ""},
    "data": b"test.txt\0186cafa3319c24956783383dc44c5cbc68c5a0ca\n",
    "parents": None,
    "children": [{"Ok": {"File": {"key": {"node": bin("186cafa3319c24956783383dc44c5cbc68c5a0ca"),
                                          "path": "test.txt"},
                                  "file_metadata": {"size": 13,
                                                    "content_id": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "content_sha1": bin("4fe2b8dd12cd9cd6a413ea960cd8c09c25f19527"),
                                                    "content_blake3": bin("7e9a0ce0d68016f0502ac50ff401830c7e2e9c894b43b242439f90f99af8835a"),
                                                    "content_sha256": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "file_header_metadata": b""}}}}],
    "tree_aux_data": None},
   {"key": {"node": bin("c8743b14e0789cc546125213c18a18d813862db5"),
            "path": ""},
    "data": b"copy.txt\017b8d4e3bafd4ec4812ad7c930aace9bf07ab033\ntest.txt\0186cafa3319c24956783383dc44c5cbc68c5a0ca\n",
    "parents": bin("15024c4dc4a27b572d623db342ae6a08d7f7adec"),
    "children": [{"Ok": {"File": {"key": {"node": bin("17b8d4e3bafd4ec4812ad7c930aace9bf07ab033"),
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
  [{"key": {"node": bin("15024c4dc4a27b572d623db342ae6a08d7f7adec"),
            "path": ""},
    "data": b"test.txt\0186cafa3319c24956783383dc44c5cbc68c5a0ca\n",
    "parents": None,
    "children": [{"Ok": {"File": {"key": {"node": bin("186cafa3319c24956783383dc44c5cbc68c5a0ca"),
                                          "path": "test.txt"},
                                  "file_metadata": {"size": 13,
                                                    "content_id": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "content_sha1": bin("4fe2b8dd12cd9cd6a413ea960cd8c09c25f19527"),
                                                    "content_blake3": bin("7e9a0ce0d68016f0502ac50ff401830c7e2e9c894b43b242439f90f99af8835a"),
                                                    "content_sha256": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "file_header_metadata": b""}}}}],
    "tree_aux_data": None},
   {"key": {"node": bin("c8743b14e0789cc546125213c18a18d813862db5"),
            "path": ""},
    "data": b"copy.txt\017b8d4e3bafd4ec4812ad7c930aace9bf07ab033\ntest.txt\0186cafa3319c24956783383dc44c5cbc68c5a0ca\n",
    "parents": bin("15024c4dc4a27b572d623db342ae6a08d7f7adec"),
    "children": [{"Ok": {"File": {"key": {"node": bin("17b8d4e3bafd4ec4812ad7c930aace9bf07ab033"),
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
  $ mononoke_newadmin derived-data -R repo derive --derived-data-types hg_augmented_manifests -i $HG_ID_1 -i $HG_ID_2 --from-predecessor
  $ hg debugapi mono:repo -e trees -f keys -f attrs --sort
  [{"key": {"node": bin("15024c4dc4a27b572d623db342ae6a08d7f7adec"),
            "path": ""},
    "data": b"test.txt\0186cafa3319c24956783383dc44c5cbc68c5a0ca\n",
    "parents": None,
    "children": [{"Ok": {"File": {"key": {"node": bin("186cafa3319c24956783383dc44c5cbc68c5a0ca"),
                                          "path": "test.txt"},
                                  "file_metadata": {"size": 13,
                                                    "content_id": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "content_sha1": bin("4fe2b8dd12cd9cd6a413ea960cd8c09c25f19527"),
                                                    "content_blake3": bin("7e9a0ce0d68016f0502ac50ff401830c7e2e9c894b43b242439f90f99af8835a"),
                                                    "content_sha256": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "file_header_metadata": b""}}}}],
    "tree_aux_data": {"augmented_manifest_id": bin("b607f9f2497ec96ef5f73f363886a3df48759ea864bff648e0f8de382236f71e"),
                      "augmented_manifest_size": 212}},
   {"key": {"node": bin("c8743b14e0789cc546125213c18a18d813862db5"),
            "path": ""},
    "data": b"copy.txt\017b8d4e3bafd4ec4812ad7c930aace9bf07ab033\ntest.txt\0186cafa3319c24956783383dc44c5cbc68c5a0ca\n",
    "parents": bin("15024c4dc4a27b572d623db342ae6a08d7f7adec"),
    "children": [{"Ok": {"File": {"key": {"node": bin("17b8d4e3bafd4ec4812ad7c930aace9bf07ab033"),
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
    "tree_aux_data": {"augmented_manifest_id": bin("f5d56263e1ffc9a4bf1e637a5d7c5245dfd89ec7b9109147cc7dcc5187e8a73f"),
                      "augmented_manifest_size": 504}}]
