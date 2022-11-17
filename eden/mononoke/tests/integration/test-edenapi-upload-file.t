# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Set up local hgrc and Mononoke config.
  $ REPONAME="test/repo"
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
  warning: failed to inspect working copy parent

Start up EdenAPI server.
  $ SEGMENTED_CHANGELOG_ENABLE=1 setup_mononoke_config
  $ start_and_wait_for_mononoke_server
Check responses.

  $ hgedenapi debugapi -e uploadfilecontents -i '[({"Sha1":"03cfd743661f07975fa2f1220c5194cbaff48451"}, b"abc\n")]'
  [{"data": {"id": {"AnyFileContentId": {"Sha1": bin("03cfd743661f07975fa2f1220c5194cbaff48451")}},
             "metadata": {"FileContentTokenMetadata": {"content_size": 4}},
             "bubble_id": None},
    "signature": {"signature": [102,
                                97,
                                107,
                                101,
                                116,
                                111,
                                107,
                                101,
                                110,
                                115,
                                105,
                                103,
                                110,
                                97,
                                116,
                                117,
                                114,
                                101]}}]

  $ hgedenapi debugapi -e ephemeralprepare -i None -i None
  [{"bubble_id": 1}]

  $ hgedenapi debugapi -e uploadfilecontents -i '[({"Sha1":"7b18d017f89f61cf17d47f92749ea6930a3f1deb"}, b"def\n")]' -i 1
  [{"data": {"id": {"AnyFileContentId": {"Sha1": bin("7b18d017f89f61cf17d47f92749ea6930a3f1deb")}},
             "metadata": {"FileContentTokenMetadata": {"content_size": 4}},
             "bubble_id": 1},
    "signature": {"signature": [102,
                                97,
                                107,
                                101,
                                116,
                                111,
                                107,
                                101,
                                110,
                                115,
                                105,
                                103,
                                110,
                                97,
                                116,
                                117,
                                114,
                                101]}}]


Check file in blobstores
  $ mononoke_newadmin filestore -R "$REPONAME" verify --content-sha1 03cfd743661f07975fa2f1220c5194cbaff48451
  content_id: true
  sha1: true
  sha256: true
  git_sha1: true
  $ mononoke_newadmin filestore -R "$REPONAME" verify --content-sha1 7b18d017f89f61cf17d47f92749ea6930a3f1deb
  Error: Content not found
  [1]
  $ mononoke_newadmin filestore -R "$REPONAME" verify --bubble-id 1 --content-sha1 7b18d017f89f61cf17d47f92749ea6930a3f1deb
  content_id: true
  sha1: true
  sha256: true
  git_sha1: true
