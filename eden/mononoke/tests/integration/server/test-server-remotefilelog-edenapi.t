# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ CACHEDIR=$PWD/cachepath
  $ . "${TEST_FIXTURES}/library.sh"

Setup repo config

  $ setup_common_config "blob_files"
  $ cd "$TESTTMP"

Setup repo

  $ hginit_treemanifest repo
  $ cd repo
  $ echo s > smallfile
  $ hg commit -Aqm "add small file"
  $ hg bookmark master_bookmark -r tip
  $ cd "$TESTTMP"

Blobimport the hg repo to Mononoke

  $ blobimport repo/.hg repo
  $ mononoke --scuba-dataset "file://$TESTTMP/log.json"
  $ wait_for_mononoke


Create a new client repository. Enable SaplingRemoteAPI there.

  $ hg clone -q mono:repo repo-clone --noupdate
  $ cd repo-clone
  $ hg pull -q -B master_bookmark
  $ hg up -q master_bookmark
  $ cat smallfile
  s

Check we didn't call getpack or gettreepack

  $ grep getpack "$TESTTMP/log.json"
  [1]
  $ grep gettreepack "$TESTTMP/log.json"
  [1]
