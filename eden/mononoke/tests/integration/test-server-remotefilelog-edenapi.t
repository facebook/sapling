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

  $ hginit_treemanifest repo-orig
  $ cd repo-orig
  $ setup_hg_server
  $ echo s > smallfile
  $ hg commit -Aqm "add small file"
  $ hg bookmark master_bookmark -r tip
  $ cd "$TESTTMP"

Blobimport the hg repo to Mononoke

  $ blobimport repo-orig/.hg repo
  $ mononoke --scuba-dataset "file://$TESTTMP/log.json"
  $ wait_for_mononoke


Create a new client repository. Enable EdenAPI there.

  $ hgclone_treemanifest ssh://user@dummy/repo-orig repo-clone --noupdate --config extensions.remotenames=
  $ cd repo-clone
  $ setup_hg_client
  $ setup_hg_edenapi
  $ hgmn pull -q -B master_bookmark
  $ hgmn up -q master_bookmark
  $ cat smallfile
  s

Check we didn't call getpack or gettreepack

  $ grep getpack "$TESTTMP/log.json"
  [1]
  $ grep gettreepack "$TESTTMP/log.json"
  [1]
