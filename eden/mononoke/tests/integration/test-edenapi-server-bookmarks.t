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


Setup testing repo for mononoke:
  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ setup_hg_server


  $ drawdag << EOS
  > B
  > |
  > A
  > EOS

  $ hg book -r $A alpha
  $ hg log -r alpha -T'{node}\n'
  426bada5c67598ca65036d57d9e4b64b0c1ce7a0
  $ hg book -r $B beta
  $ hg log -r beta -T'{node}\n'
  112478962961147124edd43549aedd1a335e44bf


import testing repo to mononoke
  $ cd ..
  $ blobimport repo-hg/.hg repo


Start up EdenAPI server.
  $ setup_mononoke_config
  $ start_edenapi_server

Create and send file data request.
  $ edenapi_make_req bookmark > req.cbor <<EOF
  > {
  >   "bookmarks": [
  >     "alpha",
  >     "beta",
  >     "unknown"
  >   ]
  > }
  > EOF
  Reading from stdin
  Generated request: WireBookmarkRequest {
      bookmarks: [
          "alpha",
          "beta",
          "unknown",
      ],
  }

  $ sslcurl -s "$EDENAPI_URI/repo/bookmarks" --data-binary @req.cbor > res.cbor

Check hgids in response.
  $ edenapi_read_res bookmark res.cbor
  Reading from file: "res.cbor"
  alpha: 426bada5c67598ca65036d57d9e4b64b0c1ce7a0
  beta: 112478962961147124edd43549aedd1a335e44bf
  unknown: Bookmark not found
