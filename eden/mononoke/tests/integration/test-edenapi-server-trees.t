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
  >   ]
  > }
  > EOF
  Reading from stdin
  Generated request: TreeRequest {
      keys: [
          Key {
              path: RepoPathBuf(
                  "",
              ),
              hgid: HgId("15024c4dc4a27b572d623db342ae6a08d7f7adec"),
          },
          Key {
              path: RepoPathBuf(
                  "",
              ),
              hgid: HgId("c8743b14e0789cc546125213c18a18d813862db5"),
          },
      ],
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

  $ edenapi_read_res tree cat res.cbor -p '' -h $ROOT_MFID_2
  Reading from file: "res.cbor"
  copy.txt\x0017b8d4e3bafd4ec4812ad7c930aace9bf07ab033 (esc)
  test.txt\x00186cafa3319c24956783383dc44c5cbc68c5a0ca (esc)
