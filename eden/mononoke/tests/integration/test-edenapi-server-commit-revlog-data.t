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
  $ echo "my commit message" > test.txt
  $ hg commit -Aqm "add test.txt"
  $ COMMIT_1=$(hg log -r . -T '{node}')
  $ hg cp test.txt copy.txt
  $ hg commit -Aqm "copy test.txt to test2.txt"
  $ COMMIT_2=$(hg log -r . -T '{node}')

Blobimport test repo.
  $ cd ..
  $ blobimport repo-hg/.hg repo

Start up EdenAPI server.
  $ setup_mononoke_config
  $ start_edenapi_server

Create and send file data request.
  $ edenapi_make_req commit-revlog-data > req.cbor <<EOF
  > {
  >   "hgids": [
  >     "$COMMIT_1",
  >     "$COMMIT_2"
  >   ]
  > }
  > EOF
  Reading from stdin
  Generated request: CommitRevlogDataRequest {
      hgids: [
          HgId("e83645968c8f2954b97a3c79ce5a6b90a464c54d"),
          HgId("c7dcf24fab3a8ab956273fa40d5cc44bc26ec655"),
      ],
  }

  $ sslcurl -s "$EDENAPI_URI/repo/commit/revlog_data" --data-binary @req.cbor > res.cbor

Check files in response.
  $ edenapi_read_res commit-revlog-data ls res.cbor
  Reading from file: "res.cbor"
  e83645968c8f2954b97a3c79ce5a6b90a464c54d
  c7dcf24fab3a8ab956273fa40d5cc44bc26ec655

Verify that filenode hashes match contents.
  $ edenapi_read_res commit-revlog-data check res.cbor
  Reading from file: "res.cbor"
  e83645968c8f2954b97a3c79ce5a6b90a464c54d matches
  c7dcf24fab3a8ab956273fa40d5cc44bc26ec655 matches

Examine file data.
  $ edenapi_read_res commit-revlog-data show --hgid e83645968c8f2954b97a3c79ce5a6b90a464c54d res.cbor
  Reading from file: "res.cbor"
  \x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x009b8fa746094652af6be3a05047424c31a48c5fac (esc)
  test
  0 0
  test.txt
  
  add test.txt (no-eol)
