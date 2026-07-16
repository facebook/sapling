# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

When the `scm/mononoke:route_original_to_augmented_hg_manifest` JK is on for a
repo, the trees endpoint serves the augmented manifest for every request --
including ones that asked for the original (non-augmented) manifest -- and fully
populates the aux metadata. If the augmented manifest is not derived, it fails
closed instead of falling back to the unprotected original manifest.

  $ . "${TEST_FIXTURES}/library.sh"

Set up local hgrc and Mononoke config.
  $ setup_common_config
  $ cd $TESTTMP

Initialize test repo. COMMIT_1/COMMIT_2 mirror test-edenapi-server-trees.t so the
augmented response below is the same; COMMIT_3 is left without an augmented
manifest to exercise the fail-closed path.
  $ hginit_treemanifest repo
  $ cd repo
  $ testtool_drawdag -R repo --print-hg-hashes --no-derive-hg-augmented <<EOF
  > COMMIT_3
  > |
  > COMMIT_2
  > |
  > COMMIT_1
  > # modify: COMMIT_1 "test.txt" "test content\n"
  > # message: COMMIT_1 "add test.txt"
  > # copy: COMMIT_2 "copy.txt" "test content\n" "COMMIT_1" "test.txt"
  > # message: COMMIT_2 "copy test.txt to test2.txt"
  > # modify: COMMIT_3 "other.txt" "other\n"
  > # message: COMMIT_3 "underived commit"
  > EOF
  COMMIT_1=* (glob)
  COMMIT_2=* (glob)
  COMMIT_3=* (glob)
  $ cd ..

Start up SaplingRemoteAPI server with route-to-augmented ON for this repo. Disable
both on-demand augmented derivation AND derive-with-hg-changeset for this repo so
COMMIT_3 stays underived and the fail-closed path below is still exercised (the
trees endpoint itself never derives).
  $ setup_mononoke_config
  $ merge_just_knobs <<EOF
  > {"bools": {"scm/mononoke:route_original_to_augmented_hg_manifest": true, "scm/mononoke:derive_hg_augmented_manifest_on_demand": false, "scm/mononoke:derive_hg_augmented_manifest_with_hg_changeset": false}}
  > EOF
  $ start_and_wait_for_mononoke_server

Pull commit metadata (no tree fetch) and read root manifest ids from the commits.
  $ cd $TESTTMP/repo
  $ cat >> .hg/hgrc <<EOF
  > [paths]
  > default = mono:repo
  > EOF
  $ hg pull -q -r $COMMIT_1 -r $COMMIT_2 -r $COMMIT_3
  $ ROOT_MFID_1=$(hg log -r $COMMIT_1 -T '{manifest}')
  $ ROOT_MFID_2=$(hg log -r $COMMIT_2 -T '{manifest}')
  $ ROOT_MFID_3=$(hg log -r $COMMIT_3 -T '{manifest}')
  $ cd $TESTTMP

Request the ORIGINAL (non-augmented) trees -- note attrs has NO augmented_trees.
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

Derive augmented manifests for COMMIT_1 and COMMIT_2.
  $ mononoke_admin derived-data -R repo derive --derived-data-types hg_augmented_manifests -i $COMMIT_1 -i $COMMIT_2 --unsafe-derive-untopologically

The original request is transparently served the augmented manifest: byte-identical
data plus tree_aux_data + has_acl + children (full upgrade), even though the client
did not request augmented_trees.
  $ hg debugapi mono:repo -e trees -f keys -f attrs --sort
  [{"key": {"node": bin("b3930c8a2f6a25b56d20ed48ce1d30cd98026792"),
            "path": ""},
    "data": b"COMMIT_1\05690dd090bcba4b8f272493af3c574cd5242c4d1\ntest.txt\0186cafa3319c24956783383dc44c5cbc68c5a0ca\n",
    "has_acl": False,
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
    "has_acl": False,
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

Test-case: original request without child metadata.
How/setup: use the same routed repo and derived COMMIT_2 manifest, but request no
parents, no child metadata, and no augmented trees.
Expectation: the route still fully populates the augmented tree metadata that
code tenting needs.
  $ cat > attrs_without_metadata << EOF
  > {
  >     "manifest_blob": True,
  >     "parents": False,
  >     "child_metadata": False,
  >     "augmented_trees": False
  > }
  > EOF
  $ cat > key2 << EOF
  > [
  >     ("", "$ROOT_MFID_2")
  > ]
  > EOF
  $ hg debugapi mono:repo -e trees -f key2 -f attrs_without_metadata --sort > "$TESTTMP/routed_without_metadata.out"
  $ python3 -c "
  > bin = lambda x: x
  > data = eval(open('$TESTTMP/routed_without_metadata.out').read())
  > entry = data[0]
  > print('has_acl=%s' % entry.get('has_acl'))
  > print('has_tree_aux_data=%s' % ('tree_aux_data' in entry))
  > print('parents_present=%s' % (entry.get('parents') is not None))
  > print('children=%s' % len(entry.get('children') or []))
  > "
  has_acl=False
  has_tree_aux_data=True
  parents_present=True
  children=4

A request for a manifest whose augmented manifest was NOT derived fails closed
(the original manifest is never served); no fallback occurs.
  $ cat > keys3 << EOF
  > [
  >     ("", "$ROOT_MFID_3")
  > ]
  > EOF
  $ hg debugapi mono:repo -e trees -f keys3 -f attrs --sort 2>&1 | grep -o "augmented Hg manifest unavailable for tented repo" | head -1
  augmented Hg manifest unavailable for tented repo
