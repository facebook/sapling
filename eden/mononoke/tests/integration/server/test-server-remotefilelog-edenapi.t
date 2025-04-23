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

  $ testtool_drawdag --print-hg-hashes -R repo --derive-all --no-default-files <<EOF
  > A
  > # modify: A "smallfile" "s\n"
  > # bookmark: A master_bookmark
  > # message: A "add small file"
  > EOF
  A=7a5f7decadde2305e44f1c94f4d8fe37c0eca02f

Start Mononoke

  $ mononoke --scuba-log-file "$TESTTMP/log.json"
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
