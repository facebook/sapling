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
  $ hg cp test.txt copy.txt
  $ hg commit -Aqm "copy test.txt to test2.txt"
  $ echo "line 2" >> test.txt
  $ echo "line 2" >> copy.txt
  $ hg commit -qm "add line 2 to test files"
  $ echo "line 3" >> test.txt
  $ echo "line 3" >> test2.txt
  $ hg commit -qm "add line 3 to test files"
  $ TEST_FILENODE=$(hg manifest --debug | grep test.txt | awk '{print $1}')
  $ COPY_FILENODE=$(hg manifest --debug | grep copy.txt | awk '{print $1}')

Blobimport test repo.
  $ cd ..
  $ blobimport repo-hg/.hg repo

Start up EdenAPI server.
  $ setup_mononoke_config
  $ start_edenapi_server

Create and send file data request.
  $ edenapi_make_req history > req.cbor <<EOF
  > {
  >   "keys": [
  >     ["test.txt", "$TEST_FILENODE"],
  >     ["copy.txt", "$COPY_FILENODE"]
  >   ]
  > }
  > EOF
  Reading from stdin
  Generated request: HistoryRequest {
      keys: [
          Key {
              path: RepoPathBuf(
                  "test.txt",
              ),
              hgid: HgId("596c909aab726d7f8b3766795239cd20ede8e125"),
          },
          Key {
              path: RepoPathBuf(
                  "copy.txt",
              ),
              hgid: HgId("672343a6daad357b926cd84a5a44a011ad029e5f"),
          },
      ],
      depth: None,
  }
  $ sslcurl -s "$EDENAPI_URI/repo/history" -d@req.cbor > res.cbor

Check history content.
  $ edenapi_read_res history show res.cbor
  Reading from file: "res.cbor"
  copy.txt:
    node: 672343a6daad357b926cd84a5a44a011ad029e5f
    parents: 17b8d4e3bafd4ec4812ad7c930aace9bf07ab033
    linknode: 6f445033ece95e6f81f0fd93cb0db7e29862888a
  
    node: 17b8d4e3bafd4ec4812ad7c930aace9bf07ab033
    parents: 186cafa3319c24956783383dc44c5cbc68c5a0ca
    linknode: 507881746c0f2eb0c6599fc8e4840d7cf45dcdbe
    copyfrom: test.txt
  
  
  test.txt:
    node: 596c909aab726d7f8b3766795239cd20ede8e125
    parents: b6fe30270546463f3630fd41fec2cd113e7a8acf
    linknode: 4af0b091e704c445e593c61b40564872773e64b3
  
    node: b6fe30270546463f3630fd41fec2cd113e7a8acf
    parents: 186cafa3319c24956783383dc44c5cbc68c5a0ca
    linknode: 6f445033ece95e6f81f0fd93cb0db7e29862888a
  
    node: 186cafa3319c24956783383dc44c5cbc68c5a0ca
    parents: None
    linknode: f91e155a86e1b909d99174818a2f98de2c128c59
  
  
