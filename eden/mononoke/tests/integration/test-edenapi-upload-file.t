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
  $ drawdag << EOF
  > B
  > |
  > A
  > EOF

import testing repo
  $ cd ..
  $ blobimport repo-hg/.hg repo

Start up EdenAPI server.
  $ SEGMENTED_CHANGELOG_ENABLE=1 setup_mononoke_config
  $ mononoke
  $ wait_for_mononoke

Create and send file data request.
  $ echo abc > file1
  $ sslcurl -X PUT -s "https://localhost:$MONONOKE_SOCKET/edenapi/repo/upload/file/sha1/$(sha1sum file1 | cut -d' ' -f1)" --data-binary @file1 > res1.cbor
  $ echo "{}" | edenapi_make_req ephemeral-prepare > req.cbor
  Reading from stdin
  Generated request: WireEphemeralPrepareRequest
  $ sslcurl -s "https://localhost:$MONONOKE_SOCKET/edenapi/repo/ephemeral/prepare" --data-binary @req.cbor > res2.cbor
  $ edenapi_read_res ephemeral-prepare res2.cbor
  Reading from file: "res2.cbor"
  Bubble id: 1
  $ echo def > file2
  $ sslcurl -X PUT -s "https://localhost:$MONONOKE_SOCKET/edenapi/repo/upload/file/sha1/$(sha1sum file2 | cut -d' ' -f1)?bubble_id=1" --data-binary @file2 > res3.cbor

Check files in response.
  $ edenapi_read_res upload-token res1.cbor
  Reading from file: "res1.cbor"
  Token 0: id AnyFileContentId(Sha1(Sha1("03cfd743661f07975fa2f1220c5194cbaff48451")))
  $ edenapi_read_res upload-token res3.cbor
  Reading from file: "res3.cbor"
  Token 0: id AnyFileContentId(Sha1(Sha1("7b18d017f89f61cf17d47f92749ea6930a3f1deb")))

Check file in blobstores
  $ mononoke_admin filestore verify sha1 03cfd743661f07975fa2f1220c5194cbaff48451
  * using repo "repo" repoid RepositoryId(0) (glob)
  * Reloading redacted config from configerator (glob)
  * content_id: true (glob)
  * sha1: true (glob)
  * sha256: true (glob)
  * git_sha1: true (glob)
  $ mononoke_admin filestore verify sha1 7b18d017f89f61cf17d47f92749ea6930a3f1deb
  * using repo "repo" repoid RepositoryId(0) (glob)
  * Reloading redacted config from configerator (glob)
  * Content not found! (glob)
  [1]
  $ mononoke_admin filestore verify --bubble-id 1 sha1 7b18d017f89f61cf17d47f92749ea6930a3f1deb
  * using repo "repo" repoid RepositoryId(0) (glob)
  * Reloading redacted config from configerator (glob)
  * content_id: true (glob)
  * sha1: true (glob)
  * sha256: true (glob)
  * git_sha1: true (glob)
