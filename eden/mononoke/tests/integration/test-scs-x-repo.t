# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-push-redirector.sh"

Setup config repo:
  $ cd "$TESTTMP"
  $ create_large_small_repo
  Setting up hg server repos
  Blobimporting them
  Adding synced mapping entry

start SCS server
  $ start_and_wait_for_scs_server

make some simple requests that we can use to check scuba logging

List repos - there should be two of them
  $ scsc repos
  large-mon
  small-mon

  $ scsc xrepo-lookup --source-repo small-mon --target-repo large-mon --bonsai-id $SMALL_MASTER_BONSAI
  bfcfb674663c5438027bcde4a7ae5024c838f76a
  $ scsc xrepo-lookup --source-repo large-mon --target-repo small-mon --bonsai-id $LARGE_MASTER_BONSAI
  11f848659bfcf77abd04f947883badd8efa88d26
