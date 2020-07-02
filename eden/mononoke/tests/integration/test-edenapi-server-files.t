# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Set up local hgrc and Mononoke config.
  $ setup_common_config
  $ cd $TESTTMP

Initialize test repo.
  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ setup_hg_server

Populate test repo
  $ echo "test content" > test.txt
  $ hg commit -Aqm "add test.txt"
  $ TEST_FILENODE=$(hg manifest --debug | grep test.txt | awk '{print $1}')
  $ hg cp test.txt copy.txt
  $ hg commit -Aqm "copy test.txt to test2.txt"
  $ COPY_FILENODE=$(hg manifest --debug | grep copy.txt | awk '{print $1}')

Blobimport test repo.
  $ cd ..
  $ blobimport repo-hg/.hg repo

Start up EdenAPI server.
  $ setup_mononoke_config
  $ start_edenapi_server

Create and send file data request.
  $ edenapi_make_req data > req.cbor <<EOF
  > {
  >   "keys": [
  >     ["test.txt", "$TEST_FILENODE"],
  >     ["copy.txt", "$COPY_FILENODE"]
  >   ]
  > }
  > EOF
  Reading from stdin
  Generated request: DataRequest {
      keys: [
          Key {
              path: RepoPathBuf(
                  "test.txt",
              ),
              hgid: HgId("186cafa3319c24956783383dc44c5cbc68c5a0ca"),
          },
          Key {
              path: RepoPathBuf(
                  "copy.txt",
              ),
              hgid: HgId("17b8d4e3bafd4ec4812ad7c930aace9bf07ab033"),
          },
      ],
  }
  $ sslcurl -s "$EDENAPI_URI/repo/files" -d@req.cbor > res.cbor

Check files in response.
  $ edenapi_read_res data ls res.cbor
  Reading from file: "res.cbor"
  186cafa3319c24956783383dc44c5cbc68c5a0ca test.txt
  17b8d4e3bafd4ec4812ad7c930aace9bf07ab033 copy.txt

Verify that filenode hashes match contents.
  $ edenapi_read_res data check res.cbor
  Reading from file: "res.cbor"

Examine file data.
  $ edenapi_read_res data cat res.cbor -p test.txt -h $TEST_FILENODE
  Reading from file: "res.cbor"
  test content

Note that copyinfo header is present for the copied file.
  $ edenapi_read_res data cat res.cbor -p copy.txt -h $COPY_FILENODE
  Reading from file: "res.cbor"
  \x01 (esc)
  copy: test.txt
  copyrev: 186cafa3319c24956783383dc44c5cbc68c5a0ca
  \x01 (esc)
  test content
