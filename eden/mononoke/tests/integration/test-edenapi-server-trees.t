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
  $ echo "test content" > test.txt
  $ hg commit -Aqm "add test.txt"
  $ ROOT_MFID_1=$(hg log -r . -T '{manifest}')
  $ hg cp test.txt copy.txt
  $ hg commit -Aqm "copy test.txt to test2.txt"
  $ ROOT_MFID_2=$(hg log -r . -T '{manifest}')

Blobimport test repo.
  $ cd ..
  $ blobimport repo-hg/.hg repo

Start up EdenAPI server.
  $ setup_mononoke_config
  $ start_edenapi_server

Create and send tree request.
  $ edenapi_make_req tree > req.cbor <<EOF
  > {
  >   "keys": [
  >     ["", "$ROOT_MFID_1"],
  >     ["", "$ROOT_MFID_2"]
  >   ],
  >   "attributes": {
  >     "manifest_blob": true,
  >     "parents": true,
  >     "child_metadata": true
  >   }
  > }
  > EOF
  Reading from stdin
  Generated request: WireTreeRequest {
      query: Some(
          ByKeys(
              WireTreeKeysQuery {
                  keys: [
                      WireKey {
                          path: WireRepoPathBuf(
                              "",
                          ),
                          hgid: WireHgId("15024c4dc4a27b572d623db342ae6a08d7f7adec"),
                      },
                      WireKey {
                          path: WireRepoPathBuf(
                              "",
                          ),
                          hgid: WireHgId("c8743b14e0789cc546125213c18a18d813862db5"),
                      },
                  ],
              },
          ),
      ),
      attributes: Some(
          WireTreeAttributesRequest {
              with_data: true,
              with_parents: true,
              with_child_metadata: true,
          },
      ),
  }
  $ sslcurl -s "$EDENAPI_URI/repo/trees" -d@req.cbor > res.cbor

Check trees in response.
  $ edenapi_read_res tree ls res.cbor
  Reading from file: "res.cbor"
  15024c4dc4a27b572d623db342ae6a08d7f7adec 
  c8743b14e0789cc546125213c18a18d813862db5 

  $ edenapi_read_res tree cat res.cbor -p '' -h $ROOT_MFID_1
  Reading from file: "res.cbor"
  test.txt\x00186cafa3319c24956783383dc44c5cbc68c5a0ca (esc)

  $ edenapi_read_res tree cat res.cbor --debug -p '' -h $ROOT_MFID_1
  Reading from file: "res.cbor"
  TreeEntry { key: Key { path: RepoPathBuf(""), hgid: HgId("15024c4dc4a27b572d623db342ae6a08d7f7adec") }, data: Some(b"test.txt\0186cafa3319c24956783383dc44c5cbc68c5a0ca\n"), parents: Some(None), children: Some([Ok(File(TreeChildFileEntry { key: Key { path: RepoPathBuf("test.txt"), hgid: HgId("186cafa3319c24956783383dc44c5cbc68c5a0ca") }, file_metadata: Some(FileMetadata { revisionstore_flags: None, content_id: Some(ContentId("888dcf533a354c23e4bf67e1ada984d96bb1089b0c3c03f4c2cb773709e7aa42")), file_type: Some(Regular), size: Some(13), content_sha1: Some(Sha1("4fe2b8dd12cd9cd6a413ea960cd8c09c25f19527")), content_sha256: Some(Sha256("a1fff0ffefb9eace7230c24e50731f0a91c62f9cefdfe77121c2f607125dffae")) }) }))]) }

  $ edenapi_read_res tree cat res.cbor -p '' -h $ROOT_MFID_2
  Reading from file: "res.cbor"
  copy.txt\x0017b8d4e3bafd4ec4812ad7c930aace9bf07ab033 (esc)
  test.txt\x00186cafa3319c24956783383dc44c5cbc68c5a0ca (esc)
